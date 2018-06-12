// Copyright (c) 2018, [Ribose Inc](https://www.ribose.com).
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
// 1. Redistributions of source code must retain the above copyright
//    notice, this list of conditions and the following disclaimer.
// 2. Redistributions in binary form must reproduce the above copyright
//    notice, this list of conditions and the following disclaimer in the
//    documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NO/T
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
// OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
// LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
// DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use health::IntervalHealthCheck;
use std::env;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

#[derive(Debug, Deserialize, Clone)]
#[serde(field_identifier, rename_all = "lowercase")]
pub enum AppAction {
    Restart,
}

#[derive(Debug)]
pub struct Application {
    pub exec: String,
    pub start: String,
    pub stop: String,
    pub restart: String,
    pub healthchecks: Vec<IntervalHealthCheck>,
    pub healthcheckfail: AppAction,
    pub state: AppState,
    pub checks: Vec<(mpsc::Sender<()>, thread::JoinHandle<()>)>,
}

#[derive(Debug, PartialEq)]
pub enum AppState {
    Running,
    Failed,
    Stopped,
}

impl Application {
    pub fn start(&mut self) -> bool {
        match Command::new(&self.exec).arg(&self.start).spawn() {
            Ok(mut child) => match child.wait() {
                Ok(s) if s.success() => {
                    log(format!("Successfully spawned {}", self.exec));
                    self.state = AppState::Running;
                    true
                }
                _ => {
                    log(format!("Application exited with error {}", self.exec));
                    self.state = AppState::Failed;
                    false
                }
            },
            Err(_) => {
                log(format!("Failed to spawn {}", self.exec));
                self.state = AppState::Failed;
                false
            }
        }
    }

    pub fn stop(&mut self) {
        let _result = Command::new(&self.exec)
            .arg(&self.stop)
            .spawn()
            .and_then(|mut c| c.wait());
        self.state = AppState::Stopped;
    }

    pub fn restart(&self) {
        let _result = Command::new(&self.exec)
            .arg(&self.restart)
            .spawn()
            .and_then(|mut c| c.wait());
    }

    pub fn spawn_check_threads<T: Send + Sync + Clone + 'static>(
        &mut self,
        fail_tx: mpsc::Sender<Option<T>>,
        fail_msg: T,
    ) -> () {
        self.checks = self.healthchecks
            .iter()
            .map(|c| {
                let check = c.clone();
                let fail_tx = fail_tx.clone();
                let fail_msg = fail_msg.clone();
                let (tx, rx) = mpsc::channel();
                let h = thread::spawn(move || {
                    let mut next = Instant::now() + check.interval;
                    loop {
                        match rx.recv_timeout(next - Instant::now()) {
                            // Ok(()) means we received the 'kill' message
                            Ok(()) => {
                                log(format!("Cancelling {}.", check.to_string()));
                                break;
                            }
                            // otherwise we timeout so proceed with the check
                            _ => {
                                next += check.interval;
                                log(format!("{}.", check.to_string()));
                                match check.do_check() {
                                    Ok(_) => (),
                                    Err(e) => {
                                        log(format!("{}. {}.", check.to_string(), e));
                                        let _t = fail_tx.send(Some(fail_msg.clone()));
                                    }
                                }
                            }
                        }
                    }
                });
                (tx, h)
            })
            .collect();
    }

    pub fn stop_check_threads(&mut self) {
        for (tx, h) in self.checks.drain(..) {
            let _t = tx.send(());
            let _t = h.join();
        }
    }
}

fn log(s: String) {
    let arg0 = env::args().next().unwrap();
    eprintln!("{}: {}", arg0, s);
}
