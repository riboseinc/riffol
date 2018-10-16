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

use libc;
use limit::{setlimit, RLimit};
use signal::signal;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::io::IntoRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use stream;

#[derive(Debug, PartialEq)]
pub enum Mode {
    Simple,
    Forking,
    OneShot,
}

#[derive(Debug)]
pub struct Application {
    pub id: String,
    pub mode: Mode,
    pub dir: String,
    pub pidfile: Option<String>,
    pub env: HashMap<String, String>,
    pub start: Vec<String>,
    pub stop: Vec<String>,
    pub healthchecks: Vec<String>,
    pub limits: Vec<RLimit>,
    pub stdout: Option<stream::Stream>,
    pub stderr: Option<stream::Stream>,
    pub state: AppState,
    pub requires: Vec<String>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum AppState {
    Idle,
    Starting {
        exec_pid: u32,
    },
    Running {
        app_pid: Option<u32>,
    },
    Stopping {
        app_pid: Option<u32>,
        exec_pid: Option<u32>,
    },
    Complete,
}

impl Application {
    pub fn start(&mut self, stream_handler: &mut stream::Handler) -> Option<u32> {
        self.start_process(&self.start)
            .map_err(|e| warn!("Failed to start application {}: {:?}", self.id, e))
            .ok()
            .and_then(|mut child| {
                if let Some(stdout) = child.stdout.take().map(|s| s.into_raw_fd()) {
                    stream_handler.add_stream(stdout, self.stdout.clone().unwrap());
                }
                if let Some(stderr) = child.stderr.take().map(|s| s.into_raw_fd()) {
                    stream_handler.add_stream(stderr, self.stderr.clone().unwrap());
                }
                match self.mode {
                    Mode::Simple => {
                        self.state = AppState::Running {
                            app_pid: Some(child.id()),
                        };
                        None
                    }
                    _ => {
                        self.state = AppState::Starting {
                            exec_pid: child.id(),
                        };
                        Some(child.id())
                    }
                }
            })
    }

    pub fn stop(&mut self) -> Option<u32> {
        let app_pid = self.get_app_pid();
        if self.mode == Mode::Simple {
            signal(app_pid.unwrap(), libc::SIGTERM);
            self.state = AppState::Stopping {
                exec_pid: None,
                app_pid,
            };
            app_pid
        } else {
            let child = self
                .start_process(&self.stop)
                .map_err(|e| warn!("Failed to stop application {}: {:?}", self.id, e))
                .ok();
            if let Some(child) = child {
                self.state = AppState::Stopping {
                    exec_pid: Some(child.id()),
                    app_pid,
                };
                Some(child.id())
            } else {
                match app_pid {
                    None | Some(0) => {
                        self.state = AppState::Idle;
                        None
                    }
                    Some(pid) => {
                        signal(pid, libc::SIGTERM);
                        self.state = AppState::Stopping {
                            exec_pid: None,
                            app_pid: Some(pid),
                        };
                        Some(pid)
                    }
                }
            }
        }
    }

    pub fn kill(&mut self, _pid: u32) {
        /* TODO */
    }

    pub fn claim_child(&mut self, child: u32, status: i32) -> bool {
        match self.state {
            AppState::Starting { exec_pid } if exec_pid == child => {
                match self.mode {
                    Mode::OneShot => {
                        if status == 0 {
                            info!("Application {} completed successfully", self.id);
                            self.state = AppState::Complete;
                        } else {
                            warn!("Application {} failed. Exit code {}", self.id, status);
                            self.state = AppState::Idle;
                        }
                    }
                    Mode::Forking => {
                        if status == 0 {
                            info!("Application {} started successfully", self.id);
                            let pid = self.read_pidfile();
                            if pid == None {
                                warn!("Couldn't read pidfile for {}", self.id);
                            }
                            self.state = AppState::Running { app_pid: pid };
                        } else {
                            warn!(
                                "Application {} failed to start. Exit code {}",
                                self.id, status
                            );
                            self.state = AppState::Idle;
                        }
                    }
                    Mode::Simple => unreachable!(),
                }
                true
            }
            AppState::Running { app_pid: pid, .. } if pid == Some(child) => {
                warn!(
                    "Application {} died unexpectedly. Exit code {}",
                    self.id, status
                );
                // This is an error regardless of exit status We need
                // to run the exec stop command but can't do it from
                // here as we'd bypass Init's timeouts so we need to
                // signal a failure and Init can clean up ... hence Some(0)
                self.state = AppState::Running { app_pid: Some(0) };
                true
            }
            AppState::Stopping { app_pid, exec_pid }
                if app_pid == Some(child) || exec_pid == Some(child) =>
            {
                if app_pid.is_none() || exec_pid.is_none() {
                    info!("Application {} stopped", self.id);
                    self.state = AppState::Idle;
                } else if app_pid == Some(child) {
                    self.state = AppState::Stopping {
                        app_pid: None,
                        exec_pid,
                    }
                } else {
                    if status != 0 {
                        warn!("Application {} stop failed. Exit code {}", self.id, status);
                    }
                    self.state = AppState::Stopping {
                        app_pid,
                        exec_pid: None,
                    }
                }
                true
            }
            _ => false,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.state == AppState::Idle
    }

    pub fn is_complete(&self) -> bool {
        self.state == AppState::Complete
    }

    pub fn is_started(&self) -> bool {
        match self.state {
            AppState::Complete | AppState::Running { .. } => true,
            _ => false,
        }
    }

    pub fn is_runaway(&self) -> bool {
        match self.state {
            AppState::Stopping { exec_pid: None, .. } => true,
            _ => false,
        }
    }

    pub fn is_dead(&self) -> bool {
        match self.state {
            AppState::Running { app_pid: Some(0) } => true,
            _ => false,
        }
    }

    fn get_app_pid(&self) -> Option<u32> {
        match self.state {
            AppState::Running { app_pid, .. } => app_pid,
            AppState::Stopping { app_pid, .. } => app_pid,
            _ => None,
        }
    }

    fn start_process(&self, args: &[String]) -> io::Result<Child> {
        fn stdio(stream: &Option<stream::Stream>) -> Stdio {
            stream
                .as_ref()
                .map(|stream| match stream {
                    stream::Stream::File { filename: f } if f == "/dev/null" => Stdio::null(),
                    _ => Stdio::piped(),
                }).unwrap_or_else(Stdio::inherit)
        }

        let limits = self.limits.clone();

        Command::new(&args[0])
            .current_dir(&self.dir)
            .env_clear()
            .envs(self.env.iter())
            .before_exec(move || {
                limits.iter().for_each(|l| setlimit(l));
                Ok(())
            }).stdout(stdio(&self.stdout))
            .stderr(stdio(&self.stderr))
            .args(&args[1..])
            .spawn()
    }

    fn read_pidfile(&self) -> Option<u32> {
        self.pidfile.as_ref().and_then(|pidfile| {
            fs::read_to_string(pidfile)
                .map_err(|e| format!("{:?}", e))
                .and_then(|s| {
                    s.lines().next().map_or_else(
                        || Err("Empty file".to_owned()),
                        |s| s.parse::<u32>().map_err(|e| format!("{:?}", e)),
                    )
                }).map_err(|e| {
                    warn!("Couldn't read pidfile ({}): {}", pidfile, e);
                }).ok()
        })
    }
}

#[cfg(test)]
mod tests {}
