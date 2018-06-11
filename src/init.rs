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
    checks: Vec<thread::JoinHandle<()>>,
}

impl Application {
    fn start(&mut self) -> bool {
        match Command::new(&self.config.exec)
            .arg(&self.config.start)
            .spawn()
        {
            Ok(mut child) => match child.wait() {
                Ok(s) if s.success() => {
                    log(format!("Successfully spawned {}", self.config.exec));
                    self.state = AppState::Running;
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
}

pub struct Init {
    applications: Vec<Arc<Mutex<Application>>>,
    thread: Option<thread::JoinHandle<()>>,
    fail_tx: Option<mpsc::Sender<Arc<Mutex<Application>>>>,
}

impl Init {
    pub fn new(mut configs: Vec<config::Application>) -> Init {
        Init {
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
            thread: None,
            fail_tx: None,
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        if let Some(_) = self.thread {
            return Err("Attempt to to call start on already running init.".to_owned());
        }

        let (tx, rx) = mpsc::channel::<Arc<Mutex<Application>>>();
        self.fail_tx = Some(tx);
        self.thread = thread::Builder::new()
            .spawn(move || for ap in rx.iter() {})
            .ok();

        if let None = self.thread {
            return Err("Failed to start the init thread.".to_owned());
        }

        // start the applications
        for ap_mutex in self.applications.iter() {
            let ref mut ap = ap_mutex.lock().unwrap();
            if !ap.start() {
                break;
            }
            // start healtcheck threads
            ap.checks = ap.config
                .healthchecks
                .iter()
                .map(|c| {
                    let am = Arc::clone(&ap_mutex);
                    let check = c.clone();
                    let tx = self.fail_tx.iter().next().unwrap().clone();
                    thread::Builder::new()
                        .spawn(move || {
                            let mut next = Instant::now() + check.interval;
                            loop {
                                thread::sleep(next - Instant::now());
                                next += check.interval;
                                log(format!("{}.", check.to_string()));
                                match check.do_check() {
                                    Ok(_) => (),
                                    Err(e) => {
                                        log(format!("{}. {}.", check.to_string(), e));
                                        let _t = tx.send(Arc::clone(&am));
                                    }
                                }
                            }
                        })
                        .unwrap()
                })
                .collect();
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
