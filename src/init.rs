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
// ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
// OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
// LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
// DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use application::{AppState, Application};
use crossbeam_channel as cc;
use libc;
use signal_hook;
use std::collections::HashMap;
use stream;

pub struct Init {
    applications: HashMap<String, Application>,
    stream_handler: stream::Handler,
    sig_recv: cc::Receiver<i32>,
    fail_recv: cc::Receiver<(String, String)>,
}

impl Init {
    pub fn run(
        applications: HashMap<String, Application>,
        sig_recv: cc::Receiver<i32>,
        fail_recv: cc::Receiver<(String, String)>,
    ) {
        Self {
            applications,
            stream_handler: stream::Handler::new(),
            sig_recv,
            fail_recv,
        }.run_loop();
    }

    fn run_loop(&mut self) {
        loop {
            if self.all_stopped() {
                break;
            }

            self.start_idle_applications();
            self.stop_stopped_applications();

            let mut signal = None;
            let mut fail: Option<(String, String)> = None;

            cc::Select::new()
                .recv(&self.sig_recv, |s| signal = s)
                .recv(&self.fail_recv, |f| fail = f)
                .wait();

            if let Some(signal) = signal {
                self.handle_signal(signal);
            }
            if let Some((group, message)) = fail {
                self.handle_fail(&group, &message)
            }
        }
    }

    fn handle_signal(&mut self, sig: i32) {
        if sig == signal_hook::SIGCHLD {
            let mut status: libc::c_int = 0;
            let pid = unsafe { libc::wait(&mut status) } as u32;
            debug!("SIGCHLD received {} {}", pid, status);
            self.applications
                .values_mut()
                .find(|app| match app.state {
                    AppState::Starting {
                        pid: p,
                        fds: _,
                        stop: _,
                    }
                        if p == pid =>
                    {
                        true
                    }
                    AppState::Stopping { pid: p, restart: _ } if p == pid => true,
                    _ => false,
                }).map(|app| match app.state {
                    AppState::Starting {
                        pid: _,
                        fds: _,
                        stop: None,
                    } => app.state = AppState::Running { stop: None },
                    AppState::Starting {
                        pid: _,
                        fds: _,
                        stop: Some(restart),
                    } => {
                        app.stop(restart).ok();
                    }
                    AppState::Stopping {
                        pid: _,
                        restart: true,
                    } => app.state = AppState::Idle,
                    AppState::Stopping {
                        pid: _,
                        restart: false,
                    } => app.state = AppState::Stopped,
                    _ => unreachable!(),
                });
        }
        unimplemented!()
    }

    fn handle_fail(&mut self, group: &str, _message: &str) {
        let ids = self
            .applications
            .keys()
            .map(|k| k.to_owned())
            .collect::<Vec<_>>();

        // find all apps subscribed to the failed healthcheck group
        let (mut failed, mut ok) = ids.into_iter().partition::<Vec<_>, _>(|id| {
            self.applications
                .get(id)
                .and_then(|a| a.healthchecks.iter().find(|h| *h == group))
                .is_some()
        });

        // add all dependencies of failed apps
        loop {
            let (mut depends, nodepends) = ok.into_iter().partition::<Vec<_>, _>(|id| {
                self.applications
                    .get(id)
                    .map(|app| {
                        app.depends
                            .iter()
                            .any(|a| failed.iter().find(|b| a == *b).is_some())
                    }).unwrap()
            });
            if depends.is_empty() {
                break;
            }
            failed.append(&mut depends);
            ok = nodepends;
        }

        // mark all as Failed
        failed.iter().for_each(|id| {
            self.applications.get_mut(id).map(|ap| {
                let new_state = match ap.state {
                    AppState::Starting {
                        pid,
                        fds,
                        stop: None,
                    } => AppState::Starting {
                        pid,
                        fds,
                        stop: Some(true),
                    },
                    AppState::Running { stop: None } => AppState::Running { stop: Some(true) },
                    ref state => state.clone(),
                };
                ap.state = new_state;
            });
        });
    }

    fn all_stopped(&self) -> bool {
        self.applications
            .values()
            .all(|a| a.state == AppState::Stopped)
    }

    fn start_idle_applications(&mut self) {
        // start idle apps
        if self
            .applications
            .values()
            .any(|a| a.state == AppState::Idle)
        {
            // get list of running apps
            let running = self
                .applications
                .iter()
                .filter(|(_, a)| a.state == AppState::Running { stop: None })
                .map(|(k, _)| k.to_owned())
                .collect::<Vec<_>>();

            self.applications
                .values_mut()
                .filter(|a| a.state == AppState::Idle)
                .for_each(|a| {
                    if a.depends
                        .iter()
                        .all(|d| running.iter().find(|r| d == *r).is_some())
                    {
                        a.start();
                    }
                });
        }
    }

    fn stop_stopped_applications(&mut self) {
        // get ids of running apps flagged with stop
        let ids = self
            .applications
            .iter()
            .filter(|(_, app)| match app.state {
                AppState::Running { stop: Some(_) } => true,
                _ => false,
            }).map(|(k, _)| k.to_owned())
            .collect::<Vec<_>>();

        // filter out those with running dependents
        let ids = ids.into_iter().filter(|id| {
            !self.applications.values().any(|app| match app.state {
                AppState::Idle | AppState::Stopped => false,
                _ => app.depends.iter().find(|d| *d == id).is_some(),
            })
        });

        // stop them
        ids.into_iter().for_each(|id| {
            self.applications.get(&id).map(|app| match app.state {
                AppState::Running {
                    stop: Some(restart),
                } => app.stop(restart),
                _ => unreachable!(),
            });
        });
    }
}
