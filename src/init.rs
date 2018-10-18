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

use application::Application;
use crossbeam_channel as cc;
use libc;
use signal_hook;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use stream;

struct InitApp {
    inner: Application,
    needs_stop: bool,
    kill_time: Option<Instant>,
    start_time: Option<Instant>,
    depends: Vec<usize>,
    rdepends: Vec<usize>,
}

impl InitApp {
    fn new(app: Application) -> Self {
        Self {
            inner: app,
            needs_stop: false,
            kill_time: None,
            start_time: None,
            depends: Vec::new(),
            rdepends: Vec::new(),
        }
    }
}

pub struct Init {
    applications: Vec<InitApp>,
}

impl Init {
    pub fn run(
        mut applications: Vec<Application>,
        sig_recv: &cc::Receiver<i32>,
        fail_recv: &cc::Receiver<(String, String)>,
    ) {
        let mut apps = Self {
            applications: applications
                .drain(..)
                .map(|app| InitApp::new(app))
                .collect(),
        };

        apps.setup_dependencies();

        let mut stream_handler = stream::Handler::new();

        let mut shutdown = false;
        while !(shutdown && apps.all_stopped()) {
            apps.do_kills();
            apps.do_stops();
            if !shutdown {
                apps.do_starts(&mut stream_handler);
            }

            let timer = apps.get_next_timeout().map(cc::after);
            let mut select = cc::Select::new()
                .recv(&sig_recv, |s| s.map(Event::Signal))
                .recv(&fail_recv, |f| f.map(Event::Fail));
            if let Some(timer) = timer.as_ref() {
                select = select.recv(timer, |t| t.map(|_| Event::Timer));
            }

            match select.wait() {
                Some(Event::Signal(signal)) => {
                    apps.handle_signal(signal);
                    shutdown = signal == signal_hook::SIGTERM || signal == signal_hook::SIGINT;
                }
                Some(Event::Fail((group, msg))) => apps.handle_healthcheck_fail(&group, &msg),
                Some(Event::Timer) => (),
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
            let child = unsafe { libc::wait(&mut status) } as u32;
            debug!("SIGCHLD received {} {}", child, status);

            let index = self
                .applications
                .iter_mut()
                .position(|app: &mut InitApp| app.inner.claim_child(child, status));

            let mut stop_idx = None;
            if let Some(idx) = index {
                let app = &mut self.applications[idx];
                if app.inner.is_dead() {
                    // The application just died unexpectedly.  We
                    // need to stop the application in order to
                    // perform any cleanup.
                    stop_idx = Some(idx);
                } else if app.inner.is_runaway() {
                    // The child was the stop process for an
                    // application and the main process is still
                    // active.  We set a kill timer in case the
                    // applicatiion doesn't die naturally
                    app.kill_time = Some(Instant::now() + Duration::from_secs(5));
                } else if app.inner.is_idle() {
                    // Application has gone idle so we can remove
                    // any pending kill timers
                    app.kill_time = None;
                    app.start_time = Some(Instant::now() + Duration::from_secs(1));
                }
            } else {
                info!("Reaped zombie with PID {}", child);
            }

            stop_idx.map_or((), |idx| self.schedule_stop(idx));
        } else if sig == signal_hook::SIGTERM || sig == signal_hook::SIGINT {
            debug!("Received termination signal ({})", sig);
            self.applications
                .iter_mut()
                .for_each(|app| app.needs_stop = true);
        }
    }

    fn handle_healthcheck_fail(&mut self, group: &str, _message: &str) {
        let fails = self.app_idxs(|app| app.inner.healthchecks.iter().any(|h| *h == group));
        fails.iter().for_each(|idx| self.schedule_stop(*idx));
    }

    fn schedule_stop(&mut self, idx: usize) {
        let mut stops = self.applications[idx].rdepends.clone();
        stops.push(idx);

        stops.iter().for_each(|idx| {
            self.applications[*idx].needs_stop = true;
        });
    }

    fn all_stopped(&self) -> bool {
        self.applications
            .iter()
            .all(|app| app.inner.is_complete() || app.inner.is_idle())
    }

    fn do_starts(&mut self, stream_handler: &mut stream::Handler) {
        let mut starts = self
            .applications
            .iter()
            .enumerate()
            .filter(|(_, app)| app.inner.is_idle())
            .filter(|(_, app)| app.start_time.map(|t| t <= Instant::now()).unwrap_or(true))
            .filter(|(_, app)| {
                app.depends.iter().all(|idx| {
                    let dep = &self.applications[*idx];
                    !dep.needs_stop && dep.inner.is_started()
                })
            }).map(|(idx, _)| idx)
            .collect::<Vec<_>>();

        starts.drain(..).for_each(|idx| {
            let app = &mut self.applications[idx];
            if app.inner.start(stream_handler) {
                app.kill_time = Some(Instant::now() + Duration::from_secs(5));
            } else if app.inner.is_idle() {
                app.start_time = Some(Instant::now() + Duration::from_secs(1));
            }
        });
    }

    fn do_stops(&mut self) {
        let stops = self
            .applications
            .iter()
            .enumerate()
            .filter(|(_, app)| app.needs_stop)
            .filter(|(_, app)| {
                app.rdepends
                    .iter()
                    .all(|idx| self.applications[*idx].inner.is_idle())
            }).map(|(idx, _)| idx)
            .collect::<Vec<_>>();

        stops.iter().for_each(|idx| {
            let app = &mut self.applications[*idx];
            if app.inner.stop() {
                app.kill_time = Some(Instant::now() + Duration::from_secs(5));
            }
            app.needs_stop = false;
        });
    }

    fn do_kills(&mut self) {
        let kills = self
            .applications
            .iter()
            .enumerate()
            .filter(|(_, app)| app.kill_time.map(|t| t <= Instant::now()).unwrap_or(false))
            .map(|(idx, _)| idx)
            .collect::<Vec<_>>();

        kills.iter().for_each(|idx| {
            self.applications[*idx].inner.kill();
            self.applications[*idx].kill_time = None;
        });
    }

    fn app_idxs<F>(&self, filter: F) -> Vec<usize>
    where
        F: Fn(&InitApp) -> bool,
    {
        self.applications
            .iter()
            .enumerate()
            .filter(|(_, app)| filter(app))
            .map(|(idx, _)| idx)
            .collect()
    }

    fn get_next_timeout(&self) -> Option<Duration> {
        let times = self.applications.iter().fold(Vec::new(), |mut times, app| {
            app.kill_time.map(|t| times.push(t));
            app.start_time.map(|t| times.push(t));
            times
        });
        times.iter().min().map(|t| *t - (Instant::now().min(*t)))
    }

    fn setup_dependencies(&mut self) {
        let mut all_deps = {
            let idxs = self
                .applications
                .iter()
                .enumerate()
                .map(|(idx, app)| (app.inner.id.as_str(), idx))
                .collect::<HashMap<_, _>>();

            self.applications
                .iter()
                .enumerate()
                .map(|(idx, _)| {
                    let (mut deps, mut others): (Vec<_>, Vec<_>) = self
                        .applications
                        .iter()
                        .enumerate()
                        .map(|(i, _)| i)
                        .partition(|&i| i == idx);
                    loop {
                        let (mut pass, fail): (Vec<_>, Vec<_>) = others.iter().partition(|&oidx| {
                            deps.iter().any(|&didx| {
                                self.applications[didx].inner.requires.iter().any(|id| {
                                    idxs.get(id.as_str())
                                        .map(|idx| idx == oidx)
                                        .unwrap_or(false)
                                })
                            })
                        });
                        if pass.is_empty() {
                            break;
                        }
                        deps.append(&mut pass);
                        others = fail;
                    }
                    deps.swap_remove(0);
                    deps
                }).collect::<Vec<_>>()
        };

        self.applications
            .iter_mut()
            .zip(all_deps.drain(..))
            .for_each(|(app, deps)| app.depends = deps);

        let mut all_rdeps = self
            .applications
            .iter()
            .enumerate()
            .map(|(app_idx, _)| {
                self.applications
                    .iter()
                    .enumerate()
                    .filter(|(_, app)| app.depends.iter().any(|&idx| idx == app_idx))
                    .map(|(idx, _)| idx)
                    .collect::<Vec<_>>()
            }).collect::<Vec<_>>();

        self.applications
            .iter_mut()
            .zip(all_rdeps.drain(..))
            .for_each(|(app, rdeps)| app.rdepends = rdeps);
    }
}
