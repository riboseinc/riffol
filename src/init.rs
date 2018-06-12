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

use config;
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;
use std::{env, thread};

#[derive(PartialEq)]
enum AppState {
    Running,
    Failed,
    Stopped,
}

struct Application {
    config: config::Application,
    state: AppState,
    checks: Vec<(mpsc::Sender<()>, thread::JoinHandle<()>)>,
}

type AppMutex = Arc<Mutex<Application>>;

impl Application {
    fn start(&mut self, fail_tx: mpsc::Sender<Option<AppMutex>>, fail_msg: AppMutex) -> bool {
        match Command::new(&self.config.exec)
            .arg(&self.config.start)
            .spawn()
        {
            Ok(mut child) => match child.wait() {
                Ok(s) if s.success() => {
                    log(format!("Successfully spawned {}", self.config.exec));
                    self.state = AppState::Running;
                    self.spawn_check_threads(fail_tx, fail_msg);
                    true
                }
                _ => {
                    log(format!(
                        "Application exited with error {}",
                        self.config.exec
                    ));
                    self.state = AppState::Failed;
                    false
                }
            },
            Err(_) => {
                log(format!("Failed to spawn {}", self.config.exec));
                self.state = AppState::Failed;
                false
            }
        }
    }

    fn spawn_check_threads(
        &mut self,
        fail_tx: mpsc::Sender<Option<AppMutex>>,
        fail_msg: AppMutex,
    ) -> () {
        self.checks = self.config
            .healthchecks
            .iter()
            .map(|c| {
                let check = c.clone();
                let fail_tx = fail_tx.clone();
                let fail_msg = Arc::clone(&fail_msg);
                let (tx, rx) = mpsc::channel();
                let h = thread::Builder::new()
                    .spawn(move || {
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
                                            let _t = fail_tx.send(Some(Arc::clone(&fail_msg)));
                                        }
                                    }
                                }
                            }
                        }
                    })
                    .unwrap();
                (tx, h)
            })
            .collect();
    }

    fn stop_check_threads(&mut self) {
        for (tx, h) in self.checks.drain(..) {
            tx.send(());
            h.join();
        }
    }
}

pub struct Init {
    applications: Vec<AppMutex>,
    fail_tx: mpsc::Sender<Option<AppMutex>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Init {
    pub fn new(mut configs: Vec<config::Application>) -> Result<Init, String> {
        let (tx, rx) = mpsc::channel::<Option<AppMutex>>();
        let h = thread::Builder::new()
            .spawn(move || {
                for msg in rx.iter() {
                    match msg {
                        Some(ap) => {
                            let ap = ap.lock().unwrap();
                        }
                        None => return (),
                    }
                }
            })
            .unwrap();

        Ok(Init {
            applications: {
                configs
                    .drain(..)
                    .map(|c| {
                        Arc::new(Mutex::new(Application {
                            config: c,
                            state: AppState::Stopped,
                            checks: vec![],
                        }))
                    })
                    .collect()
            },
            fail_tx: tx,
            thread: Some(h),
        })
    }

    pub fn start(&mut self) -> Result<(), String> {
        // start the applications
        for ap_mutex in &self.applications {
            let ap_mutex = Arc::clone(&ap_mutex);
            let tx = self.fail_tx.clone();
            let ref mut ap = ap_mutex.lock().unwrap();
            if !ap.start(tx, Arc::clone(&ap_mutex)) {
                break;
            }
        }

        if !self.applications
            .iter()
            .all(|a| a.lock().unwrap().state == AppState::Running)
        {
            self.stop();
            return Err("Some applications failed to start".to_owned());
        }

        Ok(())
    }

    pub fn stop(&mut self) {
        // stop all healtcheck threads
        for ap in &self.applications {
            ap.lock().unwrap().stop_check_threads();
        }

        // stop the applications
        for ap in self.applications
            .iter_mut()
            .filter(|a| a.lock().unwrap().state == AppState::Running)
            .rev()
        {
            let ap = ap.lock().unwrap();
            log(format!("Stopping {}", ap.config.exec));
            let _result = Command::new(&ap.config.exec)
                .arg(&ap.config.stop)
                .spawn()
                .and_then(|mut c| c.wait());
        }
    }
}

fn log(s: String) {
    let arg0 = env::args().next().unwrap();
    eprintln!("{}: {}", arg0, s);
}
