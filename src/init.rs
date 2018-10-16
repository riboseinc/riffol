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
use std::time::Duration;
use stream;
use timers::Timers;

struct InitApp {
    inner: Application,
    needs_stop: bool,
    backoff: Duration,
    depends: Vec<String>,
    rdepends: Vec<String>,
}

impl InitApp {
    fn new(app: Application) -> Self {
        Self {
            inner: app,
            needs_stop: false,
            backoff: Duration::from_secs(1),
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
        let mut stream_handler = stream::Handler::new();
        let mut timers = Timers::<(String, u32)>::new();

        while !apps.all_stopped() {
            apps.stop_stopped_applications(&mut timers);
            apps.start_idle_applications(&mut stream_handler);

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
            let child = unsafe { libc::wait(&mut status) } as u32;
            debug!("SIGCHLD received {} {}", child, status);

            let index = self
                .applications
                .iter_mut()
                .position(|app: &mut InitApp| app.inner.claim_child(child, status));

            if let Some(app) = index.map(|n| self.applications.get_mut(n).unwrap()) {
                if app.inner.is_dead() {
                    // The application just died unexpectedly so
                    // we need to stop the application in order to
                    // perform any cleanup.
                    app.needs_stop = true;
                } else if app.inner.is_runaway() {
                    // The child was the stop process for an
                    // application and the main process is still
                    // active.  We set a kill timer in case the
                    // applicatiion doesn't die naturally
                    // TODO ...
                    // timers.add_timer(Duration::from_secs(5), Action::Kill(app));
                } else if app.inner.is_idle() {
                    // Application has gone idle so we can remove
                    // any pending kill timers
                    // TODO ...
                }
            } else {
                info!("Reaped zombie with PID {}", child);
            }
        } else if sig == signal_hook::SIGTERM || sig == signal_hook::SIGINT {
            debug!("Received termination signal ({})", sig);
            self.applications
                .iter_mut()
                .for_each(|app| app.needs_stop = true)
        }
    }

    fn handle_healthcheck_fail(&mut self, group: &str, _message: &str) {
        self.app_ids(|app| app.inner.healthchecks.iter().any(|h| *h == group))
            .iter()
            .for_each(|id| self.fail_app(id));
    }

    fn handle_timeout(&mut self, timers: &mut Timers<(String, u32)>) {
        let payload = timers.remove_earliest();
        let app = self.get_app_mut(&payload.0);
        app.inner.kill(payload.1);
    }

    fn fail_app(&mut self, id: &str) {
        let mut failed = vec![id.to_owned()];
        let mut remaining = self.app_ids(|app| app.inner.id != id);

        // add all dependents of failed app
        loop {
            let (mut depends, others) = remaining.into_iter().partition::<Vec<_>, _>(|id| {
                self.get_app(id)
                    .inner
                    .requires
                    .iter()
                    .any(|a| failed.iter().any(|b| a == b))
            });
            if depends.is_empty() {
                break;
            }
            failed.append(&mut depends);
            remaining = others;
        }

        // mark all as Failed
        failed.iter().for_each(|_id| () /* TODO stop list */);
    }

    fn all_stopped(&self) -> bool {
        self.applications
            .iter()
            .all(|app| app.inner.is_complete() || app.inner.is_idle())
    }

    fn start_idle_applications(&mut self, stream_handler: &mut stream::Handler) {
        if self.applications.iter().any(|app| app.inner.is_idle()) {
            // get ids of running or completed OneShot apps
            let running = self.app_ids(|app| app.inner.is_started());

            self.applications
                .iter_mut()
                .filter(|app| app.inner.is_idle())
                .for_each(|app| {
                    if app
                        .inner
                        .requires
                        .iter()
                        .all(|d| running.iter().any(|r| d == r))
                    {
                        app.inner.start(stream_handler);
                        /* TODO set timer */
                    }
                });
        }
    }

    fn stop_stopped_applications(&mut self, timers: &mut Timers<(String, u32)>) {
        if self.applications.iter().any(|app| app.needs_stop) {
            // make a list of all dependencies of running apps
            let depends = self.applications.iter().fold(Vec::new(), |mut ds, app| {
                if !(app.inner.is_idle() || app.inner.is_complete()) {
                    app.depends.iter().for_each(|d| ds.push(d.to_owned()));
                }
                ds
            });

            // call stop on any running application scheduled to stop
            // on which no other running application depends
            self.applications.iter_mut().for_each(|app| {
                if app.needs_stop && !depends.iter().any(|d| d == &app.inner.id) {
                    if let Some(pid) = app.inner.stop() {
                        timers.add_timer(Duration::from_secs(5), (app.inner.id.to_owned(), pid));
                    }
                }
            });
        }
    }

    fn app_ids<F>(&self, filter: F) -> Vec<String>
    where
        F: Fn(&InitApp) -> bool,
    {
        self.applications
            .iter()
            .filter(|app| filter(app))
            .map(|app| app.inner.id.to_owned())
            .collect()
    }

    fn get_app(&self, id: &str) -> &InitApp {
        self.applications
            .iter()
            .find(|app| app.inner.id == id)
            .expect("No such application id")
    }

    fn get_app_mut(&mut self, id: &str) -> &mut InitApp {
        self.applications
            .iter_mut()
            .find(|app| app.inner.id == id)
            .expect("No such application id")
    }
}
