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

use application::{AppState, Application, Mode};
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
                self.handle_healthcheck_fail(&group, &message)
            }
        }
    }

    fn handle_signal(&mut self, sig: i32) {
        if sig == signal_hook::SIGCHLD {
            let mut status: libc::c_int = 0;
            let pid = unsafe { libc::wait(&mut status) } as u32;
            debug!("SIGCHLD received {} {}", pid, status);
            let mut failed_id = None;
            if let Some((id, app)) = self
                .applications
                .iter_mut()
                .find(|(_, app)| match app.state {
                    AppState::Starting { pid: p, .. } if p == pid => true,
                    AppState::Running { pid: p, .. } if p == Some(pid) => true,
                    AppState::Stopping { pid: p, .. } if p == pid => true,
                    _ => false,
                }) {
                debug!("Application ({}) process exited with code {}", id, status);
                match app.state {
                    AppState::Starting { stop, .. } => match app.mode {
                        Mode::OneShot | Mode::Forking if status != 0 => {
                            warn!("Application {} failed to start. Exit code {}", id, status);
                            app.state = AppState::Idle;
                        }
                        Mode::OneShot => app.state = AppState::Stopped,
                        Mode::Forking => {
                            if let Some(restart) = stop {
                                app.stop(restart).ok();
                            } else {
                                app.state = AppState::Running {
                                    stop: None,
                                    pid: app.read_pidfile(),
                                };
                            }
                        }
                        _ => unreachable!(),
                    },
                    AppState::Running { stop, .. } => {
                        warn!(
                            "Application {} stopped unexpectedly. Exit code {}",
                            id, status
                        );
                        match stop {
                            Some(true) => app.state = AppState::Idle,
                            Some(false) => app.state = AppState::Stopped,
                            None => {
                                // unexpected termination, restart dependents
                                app.state = AppState::Idle;
                                failed_id = Some(id.clone());
                            }
                        }
                    }
                    AppState::Stopping { restart: true, .. } => app.state = AppState::Idle,
                    AppState::Stopping { restart: false, .. } => app.state = AppState::Stopped,
                    _ => unreachable!(),
                }
            }
            if let Some(id) = failed_id {
                self.fail_dependents(&id);
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

    fn handle_healthcheck_fail(&mut self, group: &str, _message: &str) {
        self.applications
            .iter()
            .filter(|(_, app)| app.healthchecks.iter().any(|h| *h == group))
            .map(|(id, _)| id.to_owned())
            .collect::<Vec<_>>()
            .iter()
            .for_each(|id| {
                self.fail_app(id);
                self.fail_dependents(id);
            });
    }

    fn fail_app(&mut self, id: &str) {
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
                AppState::Running { stop: None, pid } => {
                    app.state = AppState::Running {
                        stop: Some(true),
                        pid,
                    }
                }
                _ => (),
            };
        }
    }

    fn fail_dependents(&mut self, id: &str) {
        let mut failed = vec![id.to_owned()];
        let mut remaining = self
            .applications
            .keys()
            .filter(|k| k.as_str() != id)
            .map(|k| k.to_owned())
            .collect::<Vec<_>>();

        // add all dependents of failed app
        loop {
            let (mut depends, others) = remaining.into_iter().partition::<Vec<_>, _>(|id| {
                self.applications
                    .get(id)
                    .map(|app| app.depends.iter().any(|a| failed.iter().any(|b| a == b)))
                    .unwrap()
            });
            if depends.is_empty() {
                break;
            }
            failed.append(&mut depends);
            remaining = others;
        }

        // mark all as Failed
        failed.iter().for_each(|id| self.fail_app(id));
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
            // get list of running or completed OneShot apps
            let running = self
                .applications
                .iter()
                .filter(|(_, ap)| match ap.state {
                    AppState::Running { stop: None, .. } => true,
                    AppState::Stopped if ap.mode == Mode::OneShot => true,
                    _ => false,
                }).map(|(k, _)| k.to_owned())
                .collect::<Vec<_>>();

            let mut streams = Vec::new();
            self.applications
                .values_mut()
                .filter(|a| a.state == AppState::Idle)
                .for_each(|ap| {
                    if ap.depends.iter().all(|d| running.iter().any(|r| d == r)) {
                        ap.start().ok();
                        if let AppState::Starting {
                            fds: (_, stdout, stderr),
                            pid,
                            ..
                        } = ap.state
                        {
                            if let Some(stdout) = stdout {
                                streams.push((stdout, ap.stdout.clone()));
                            }
                            if let Some(stderr) = stderr {
                                streams.push((stderr, ap.stderr.clone()));
                            }
                            if ap.mode == Mode::Simple {
                                ap.state = AppState::Running {
                                    stop: None,
                                    pid: Some(pid),
                                };
                            }
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
                AppState::Running { stop: Some(_), .. } => true,
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
                    ..
                } => app.stop(restart),
                _ => unreachable!(),
            });
        });
    }
}
