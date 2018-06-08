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
use health::IntervalHealthCheck;
use std::process::Command;
use std::time::{Duration, Instant};
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
}

pub struct Init {
    applications: Vec<Application>,
}

struct Check<'a> {
    the_check: &'a IntervalHealthCheck,
    application: &'a Application,
    instant: Instant,
}

impl Init {
    pub fn new(mut configs: Vec<config::Application>) -> Init {
        Init {
            applications: {
                configs
                    .drain(..)
                    .map(|c| Application {
                        config: c,
                        state: AppState::Stopped,
                    })
                    .collect()
            },
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        let arg0 = env::args().next().unwrap();

        for mut ap in self.applications.iter_mut() {
            match Command::new(&ap.config.exec).arg(&ap.config.start).spawn() {
                Ok(mut child) => match child.wait() {
                    Ok(s) if s.success() => {
                        eprintln!("{}: Successfully spawned {}", arg0, ap.config.exec);
                        ap.state = AppState::Running;
                    }
                    _ => {
                        eprintln!("{}: Application exited with error {}", arg0, ap.config.exec);
                        ap.state = AppState::Failed;
                    }
                },
                Err(_) => {
                    eprintln!("{}: Failed to spawn {}", arg0, ap.config.exec);
                    ap.state = AppState::Failed;
                    break;
                }
            }
        }

        if !self.applications
            .iter()
            .all(|a| a.state == AppState::Running)
        {
            self.stop();
            return Err("Some applications failed to start".to_owned());
        }

        // build vector of Checks
        let mut checks = self.applications.iter().fold(vec![], |mut a, v| {
            for c in v.config.healthchecks.iter() {
                a.push(Check {
                    application: v,
                    the_check: c,
                    instant: Instant::now() + c.interval,
                });
            }
            a
        });

        /*
        loop {
            let now = Instant::now();
            match checks.iter_mut().min_by(|x, y| x.instant.cmp(&y.instant)) {
                Some(check) => {
                    if now <= check.instant {
                        thread::sleep(check.instant - now);
                    }
                    check.instant += check.the_check.interval;
                    if !check.the_check.do_check() {}
                }
                _ => thread::sleep(Duration::from_secs(1)),
            }
        }
         */
        Ok(())
    }

    pub fn stop(&mut self) {
        let arg0 = env::args().next().unwrap();

        for ap in self.applications
            .iter_mut()
            .filter(|a| a.state == AppState::Running)
            .rev()
        {
            eprintln!("{}: Stopping {}", arg0, ap.config.exec);
            let _result = Command::new(&ap.config.exec)
                .arg(&ap.config.stop)
                .spawn()
                .and_then(|mut c| c.wait());
        }
    }
}
