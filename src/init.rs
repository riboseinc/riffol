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
            if let Some(app) = self.applications.values_mut().find(|app| match app.state {
                AppState::Starting { pid: p, .. } if p == pid => true,
                AppState::Stopping { pid: p, .. } if p == pid => true,
                _ => false,
            }) {
                match app.state {
                    AppState::Starting { stop, .. } => {
                        if let Some(restart) = stop {
                            app.stop(restart).ok();
                        } else {
                            app.state = AppState::Running { stop: None };
                        }
                    }
                    AppState::Stopping { restart: true, .. } => app.state = AppState::Idle,
                    AppState::Stopping { restart: false, .. } => app.state = AppState::Stopped,
                    _ => unreachable!(),
                }
            }
        } else if sig == signal_hook::SIGTERM || sig == signal_hook::SIGINT {
            debug!("Received termination signal ({})", sig);
            self.applications
                .values_mut()
                .for_each(|app| match app.state {
                    AppState::Idle => app.state = AppState::Stopped,
                    AppState::Starting { pid, fds, .. } => {
                        app.state = AppState::Starting {
                            pid,
                            fds,
                            stop: Some(false),
                        }
                    }
                    AppState::Running { .. } => {
                        app.stop(false).ok();
                    }
                    AppState::Stopping { pid, .. } => {
                        app.state = AppState::Stopping {
                            pid,
                            restart: false,
                        }
                    }
                    AppState::Stopped => (),
                })
        }
    }

    fn handle_fail(&mut self, group: &str, _message: &str) {
        let ids = self
            .applications
            .keys()
            .map(|k| k.to_owned())
            .collect::<Vec<_>>();

        // find all apps subscribed to the failed healthcheck group
        // ignore apps that aren't running
        let (mut failed, mut ok) = ids.into_iter().partition::<Vec<_>, _>(|id| {
            self.applications
                .get(id)
                .filter(|a| match a.state {
                    AppState::Running { stop: None } => true,
                    _ => false,
                }).and_then(|a| a.healthchecks.iter().find(|h| *h == group))
                .is_some()
        });

        // add all dependents of failed apps
        loop {
            let (mut depends, nodepends) = ok.into_iter().partition::<Vec<_>, _>(|id| {
                self.applications
                    .get(id)
                    .map(|app| app.depends.iter().any(|a| failed.iter().any(|b| a == b)))
                    .unwrap()
            });
            if depends.is_empty() {
                break;
            }
            failed.append(&mut depends);
            ok = nodepends;
        }

        // mark all as Failed
        failed.iter().for_each(|id| {
            if let Some(app) = self.applications.get_mut(id) {
                match app.state {
                    AppState::Starting {
                        pid,
                        fds,
                        stop: None,
                    } => {
                        app.state = AppState::Starting {
                            pid,
                            fds,
                            stop: Some(true),
                        }
                    }
                    AppState::Running { stop: None } => {
                        app.state = AppState::Running { stop: Some(true) }
                    }
                    _ => (),
                };
            }
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

            let mut streams = Vec::new();
            self.applications
                .values_mut()
                .filter(|a| a.state == AppState::Idle)
                .for_each(|ap| {
                    if ap.depends.iter().all(|d| running.iter().any(|r| d == r)) {
                        ap.start().ok();
                        if let AppState::Starting {
                            fds: (_, Some(stdout), _),
                            ..
                        } = ap.state
                        {
                            streams.push((stdout, ap.stdout.clone()));
                        }
                        if let AppState::Starting {
                            fds: (_, _, Some(stderr)),
                            ..
                        } = ap.state
                        {
                            streams.push((stderr, ap.stderr.clone()));
                        }
                    }
                });
            streams
                .drain(..)
                .for_each(|(fd, s)| self.stream_handler.add_stream(fd, s.unwrap()));
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
        let ids = ids
            .into_iter()
            .filter(|id| {
                !self.applications.values().any(|app| match app.state {
                    AppState::Idle | AppState::Stopped => false,
                    _ => app.depends.iter().any(|d| d == id),
                })
            }).collect::<Vec<_>>();

        // stop them
        ids.into_iter().for_each(|id| {
            self.applications.get_mut(&id).map(|app| match app.state {
                AppState::Running {
                    stop: Some(restart),
                } => app.stop(restart),
                _ => unreachable!(),
            });
        });
    }
}
