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
use signal::signal;
use signal_hook;
use stream;
use timers::Timers;

pub struct Init {
    applications: Vec<Application>,
}

impl Init {
    pub fn run(
        applications: Vec<Application>,
        sig_recv: cc::Receiver<i32>,
        fail_recv: cc::Receiver<(String, String)>,
    ) {
        let mut apps = Self { applications };
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

            if !self
                .applications
                .iter_mut()
                .any(|app| app.claim_child(child, status))
            {
                info!("Reaped zombie with PID {}", child);
            }
        } else if sig == signal_hook::SIGTERM || sig == signal_hook::SIGINT {
            debug!("Received termination signal ({})", sig);
            self.applications
                .iter_mut()
                .for_each(|app| app.schedule_stop(false))
        }
    }

    fn handle_healthcheck_fail(&mut self, group: &str, _message: &str) {
        self.app_ids(|app| app.healthchecks.iter().any(|h| *h == group))
            .iter()
            .for_each(|id| self.fail_app(id));
    }

    fn handle_timeout(&mut self, timers: &mut Timers<(String, u32)>) {
        let payload = timers.remove_earliest();
        let app = self.get_app(&payload.0);
        if app.is_stopping() && app.get_service_pid() == Some(payload.1) {
            signal(payload.1, libc::SIGKILL);
        }
    }

    fn fail_app(&mut self, id: &str) {
        let mut failed = vec![id.to_owned()];
        let mut remaining = self.app_ids(|app| app.id != id);

        // add all dependents of failed app
        loop {
            let (mut depends, others) = remaining.into_iter().partition::<Vec<_>, _>(|id| {
                self.get_app(id)
                    .depends
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
        failed
            .iter()
            .for_each(|id| self.get_app_mut(id).schedule_stop(true));
    }

    fn all_stopped(&self) -> bool {
        self.applications.iter().all(|app| app.is_stopped())
    }

    fn start_idle_applications(&mut self, stream_handler: &mut stream::Handler) {
        if self.applications.iter().any(|app| app.is_idle()) {
            // get ids of running or completed OneShot apps
            let running = self.app_ids(|app| app.is_running() || app.is_complete());

            self.applications
                .iter_mut()
                .filter(|app| app.is_idle())
                .for_each(|app| {
                    if app.depends.iter().all(|d| running.iter().any(|r| d == r)) {
                        if let Err(e) = app.start(stream_handler) {
                            warn!("Failed to start application ({}): {}", app.id, e);
                        }
                    }
                });
        }
    }

    fn stop_stopped_applications(&mut self, _timers: &mut Timers<(String, u32)>) {
        if self.applications.iter().any(|app| app.is_scheduled_stop()) {
            // make a list of all dependencies of running apps
            let depends = self.applications.iter().fold(Vec::new(), |mut ds, app| {
                if !(app.is_idle() || app.is_stopped()) {
                    app.depends.iter().for_each(|d| ds.push(d.to_owned()));
                }
                ds
            });

            // call stop on any running application scheduled to stop
            // on which no other running application depends
            self.applications.iter_mut().for_each(|app| {
                if app.is_scheduled_stop() {
                    if !depends.iter().any(|d| d == &app.id) {
                        if let Err(e) = app.stop() {
                            warn!("Failed to stop application ({}): {}", app.id, e);
                        }
                    }
                }
            });
        }
    }

    fn app_ids<F>(&self, filter: F) -> Vec<String>
    where
        F: Fn(&Application) -> bool,
    {
        self.applications
            .iter()
            .filter(|app| filter(app))
            .map(|app| app.id.to_owned())
            .collect()
    }

    fn get_app(&self, id: &str) -> &Application {
        self.applications
            .iter()
            .find(|app| app.id == id)
            .expect("No such application id")
    }

    fn get_app_mut(&mut self, id: &str) -> &mut Application {
        self.applications
            .iter_mut()
            .find(|app| app.id == id)
            .expect("No such application id")
    }
}
