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

use application::{AppAction, AppState, Application};
use std::sync::{mpsc, Arc, Mutex};
use std::{env, thread};

type AppMutex = Arc<Mutex<Application>>;

pub struct Init {
    applications: Vec<AppMutex>,
    fail_tx: mpsc::Sender<Option<AppMutex>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Init {
    pub fn new(mut applications: Vec<Application>) -> Init {
        let (tx, rx) = mpsc::channel::<Option<AppMutex>>();
        let fail_tx = tx.clone();
        let healthcheck_fn = move || {
            for msg in rx.iter() {
                if let Some(ap_mutex) = msg {
                    let mut ap = ap_mutex.lock().unwrap();
                    match ap.healthcheckfail {
                        AppAction::Restart => {
                            ap.stop_check_threads();
                            ap.restart();
                            ap.spawn_check_threads(tx.clone(), Arc::clone(&ap_mutex));
                        }
                    }
                } else {
                    // None signals return
                    return ();
                }
            }
        };

        Init {
            applications: applications
                .drain(..)
                .map(|app| Arc::new(Mutex::new(app)))
                .collect(),
            fail_tx: fail_tx,
            thread: Some(thread::spawn(healthcheck_fn)),
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        // start the applications
        if self.applications
            .iter()
            .map(|ap_mutex| {
                let mut ap = ap_mutex.lock().unwrap();
                if !ap.start() {
                    return false;
                }
                ap.spawn_check_threads(self.fail_tx.clone(), Arc::clone(&ap_mutex));
                true
            })
            .any(|b| !b)
        {
            self.stop();
            return Err("Some applications failed to start".to_owned());
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        // stop all healtcheck threads
        self.applications.iter().for_each(|ap_mutex| {
            ap_mutex.lock().unwrap().stop_check_threads();
        });

        // stop healthcheck synchronisation thread
        let _t = self.fail_tx.send(None);
        let _t = self.thread.take().unwrap().join();

        // stop the applications
        self.applications.iter().rev().for_each(|ap_mutex| {
            let mut ap = ap_mutex.lock().unwrap();
            if ap.state == AppState::Running {
                log(format!("Stopping {}", ap.exec));
                ap.stop();
            }
        });
    }
}

fn log(s: String) {
    let arg0 = env::args().next().unwrap();
    eprintln!("{}: {}", arg0, s);
}
