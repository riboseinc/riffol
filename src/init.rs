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
use signal::signal;
use signal_hook;
use std::collections::HashMap;
use std::time::Duration;
use stream;
use timers::Timers;

pub struct Init {
    applications: HashMap<String, Application>,
}

impl Init {
    pub fn run(
        applications: HashMap<String, Application>,
        sig_recv: cc::Receiver<i32>,
        fail_recv: cc::Receiver<(String, String)>,
    ) {
        let mut apps = Self { applications };
        let mut stream_handler = stream::Handler::new();
        let mut timers = Timers::<(String, u32)>::new();

        while !apps.all_stopped() {
            apps.start_idle_applications(&mut stream_handler);
            apps.stop_stopped_applications(&mut timers);

            let timer = timers.get_timeout().map(cc::after);
            let mut select = cc::Select::new()
                .recv(&sig_recv, |s| s.map(Event::Signal))
                .recv(&fail_recv, |f| f.map(Event::Fail));
            if let Some(timer) = timer.as_ref() {
                select = select.recv(timer, |t| t.map(|_| Event::Timer));
            }

            match select.wait() {
                Some(Event::Signal(signal)) => apps.handle_signal(signal),
                Some(Event::Fail((group, msg))) => apps.handle_healthcheck_fail(&group, &msg),
                Some(Event::Timer) => apps.handle_timeout(&mut timers),
                None => unreachable!(),
            }

            enum Event {
                Signal(i32),
                Fail((String, String)),
                Timer,
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
                            if let Some(false) = stop {
                                app.state = AppState::Stopped;
                            } else {
                                app.state = AppState::Idle;
                            }
                        }
                        Mode::OneShot => app.state = AppState::Stopped,
                        Mode::Forking => {
                            if let Some(restart) = stop {
                                if let Err(e) = app.stop(restart) {
                                    warn!("Application {} stop failed: {}", id, e);
                                }
                            } else {
                                app.state = AppState::Running {
                                    stop: None,
                                    pid: app.read_pidfile(),
                                };
                            }
                        }
                        Mode::Simple => unreachable!(),
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
                    AppState::Stopped | AppState::Idle => unreachable!(),
                }
            }
            if let Some(id) = failed_id {
                self.fail_app(&id);
            }
        } else if sig == signal_hook::SIGTERM || sig == signal_hook::SIGINT {
            debug!("Received termination signal ({})", sig);
            self.applications
                .iter_mut()
                .for_each(|(_, app)| app.schedule_stop(false))
        }
    }

    fn handle_healthcheck_fail(&mut self, group: &str, _message: &str) {
        self.app_ids(|_, app| app.healthchecks.iter().any(|h| *h == group))
            .iter()
            .for_each(|id| self.fail_app(id));
    }

    fn handle_timeout(&mut self, timers: &mut Timers<(String, u32)>) {
        let payload = timers.remove_earliest();
        if let Some(app) = self.applications.get(&payload.0) {
            if let AppState::Stopping { pid, .. } = app.state {
                if pid == payload.1 {
                    signal(pid, libc::SIGKILL);
                }
            }
        }
    }

    fn fail_app(&mut self, id: &str) {
        let mut failed = vec![id.to_owned()];
        let mut remaining = self.app_ids(|k, _| k != id);

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
        failed.iter().for_each(|id| {
            self.applications
                .get_mut(id)
                .map(|app| app.schedule_stop(true));
        });
    }

    fn all_stopped(&self) -> bool {
        self.applications
            .values()
            .all(|a| a.state == AppState::Stopped)
    }

    fn start_idle_applications(&mut self, stream_handler: &mut stream::Handler) {
        if self
            .applications
            .values()
            .any(|a| a.state == AppState::Idle)
        {
            // get ids of running or completed OneShot apps
            let running = self.app_ids(|_, app| match app.state {
                AppState::Running { stop: None, .. } => true,
                AppState::Stopped if app.mode == Mode::OneShot => true,
                _ => false,
            });

            self.applications
                .iter_mut()
                .filter(|(_, app)| app.state == AppState::Idle)
                .for_each(|(id, app)| {
                    if app.depends.iter().all(|d| running.iter().any(|r| d == r)) {
                        if let Err(e) = app.start() {
                            warn!("Failed to stop application ({}): {}", id, e);
                        } else {
                            if let AppState::Starting {
                                fds: (_, stdout, stderr),
                                pid,
                                ..
                            } = app.state
                            {
                                if let Some(stdout) = stdout {
                                    stream_handler.add_stream(stdout, app.stdout.clone().unwrap());
                                }
                                if let Some(stderr) = stderr {
                                    stream_handler.add_stream(stderr, app.stderr.clone().unwrap());
                                }
                                if app.mode == Mode::Simple {
                                    app.state = AppState::Running {
                                        stop: None,
                                        pid: Some(pid),
                                    };
                                }
                            }
                        }
                    }
                });
        }
    }

    fn stop_stopped_applications(&mut self, timers: &mut Timers<(String, u32)>) {
        if self.applications.values().any(|app| match app.state {
            AppState::Running { stop: Some(_), .. } => true,
            _ => false,
        }) {
            // make a list of all dependencies of running apps
            let depends = self.applications.values().fold(Vec::new(), |mut ds, app| {
                if !(app.state == AppState::Idle || app.state == AppState::Stopped) {
                    app.depends.iter().for_each(|d| ds.push(d.to_owned()));
                }
                ds
            });

            // call stop on any running application scheduled to stop
            // on which no other running application depends
            self.applications.iter_mut().for_each(|(id, app)| {
                if let AppState::Running {
                    pid,
                    stop: Some(restart),
                } = app.state
                {
                    if !depends.iter().any(|d| d == id) {
                        if let Err(e) = app.stop(restart) {
                            warn!("Failed to stop application ({}): {}", id, e);
                        } else {
                            if app.mode == Mode::Simple {
                                timers.add_timer(
                                    Duration::from_secs(5),
                                    (id.to_owned(), pid.unwrap()),
                                );
                            }
                        }
                    }
                }
            });
        }
    }

    fn app_ids<F>(&self, filter: F) -> Vec<String>
    where
        F: Fn(&str, &Application) -> bool,
    {
        self.applications
            .iter()
            .filter(|(id, app)| filter(id, app))
            .map(|(id, _)| id.to_owned())
            .collect()
    }
}
