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
    pub depends: Vec<String>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum AppState {
    Idle,
    Starting {
        pid: u32,
        stop: Option<bool>,
    },
    Running {
        stop: Option<bool>,
        pid: Option<u32>,
    },
    Stopping {
        pid: Option<u32>,
        service_pid: Option<u32>,
        restart: bool,
    },
    Stopped,
}

impl Application {
    pub fn start(&mut self, stream_handler: &mut stream::Handler) -> io::Result<()> {
        self.start_process(&self.start).map(|mut child| {
            if let Some(stdout) = child.stdout.take().map(|s| s.into_raw_fd()) {
                stream_handler.add_stream(stdout, self.stdout.clone().unwrap());
            }
            if let Some(stderr) = child.stderr.take().map(|s| s.into_raw_fd()) {
                stream_handler.add_stream(stderr, self.stderr.clone().unwrap());
            }
            self.state = match self.mode {
                Mode::Simple => AppState::Running {
                    stop: None,
                    pid: Some(child.id()),
                },
                _ => AppState::Starting {
                    pid: child.id(),
                    stop: None,
                },
            }
        })
    }

    pub fn stop(&mut self) -> io::Result<()> {
        if let AppState::Running {
            pid: Some(pid),
            stop: Some(restart),
        } = self.state
        {
            if self.mode == Mode::Simple {
                signal(pid, libc::SIGTERM);
                self.state = AppState::Stopping {
                    pid: None,
                    service_pid: Some(pid),
                    restart,
                };
                Ok(())
            } else {
                self.start_process(&self.stop).map(|child| {
                    self.state = AppState::Stopping {
                        pid: Some(child.id()),
                        service_pid: Some(pid),
                        restart,
                    };
                })
            }
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "Application not running",
            ))
        }
    }

    pub fn schedule_stop(&mut self, restart: bool) {
        match self.state {
            AppState::Idle if !restart => self.state = AppState::Stopped,
            AppState::Starting { pid, stop } if stop != Some(false) => {
                self.state = AppState::Starting {
                    pid,
                    stop: Some(restart),
                };
            }
            AppState::Running { pid, stop } if stop != Some(false) => {
                self.state = AppState::Running {
                    pid,
                    stop: Some(restart),
                };
            }
            AppState::Stopping {
                service_pid,
                pid,
                restart: true,
            } => {
                self.state = AppState::Stopping {
                    service_pid,
                    pid,
                    restart,
                };
            }
            _ => (),
        }
    }

    pub fn claim_child(&mut self, child: u32, status: i32) -> bool {
        match self.state {
            AppState::Starting { pid, stop, .. } if pid == child => {
                match self.mode {
                    Mode::OneShot | Mode::Forking if status != 0 => {
                        warn!(
                            "Application {} failed to start. Exit code {}",
                            self.id, status
                        );
                        if let Some(false) = stop {
                            self.state = AppState::Stopped;
                        } else {
                            self.state = AppState::Idle;
                        }
                    }
                    Mode::OneShot => self.state = AppState::Stopped,
                    Mode::Forking => {
                        if let Some(pid) = self.read_pidfile() {
                            self.state = AppState::Running {
                                stop: stop,
                                pid: Some(pid),
                            };
                        } else {
                            warn!("Couldn't read pidfile for {}", self.id);
                            if let Err(e) = self.stop() {
                                warn!("Application {} stop failed: {}", self.id, e);
                            }
                        }
                    }
                    Mode::Simple => unreachable!(),
                }
                true
            }
            AppState::Running { stop, .. } => {
                warn!(
                    "Application {} stopped unexpectedly. Exit code {}",
                    self.id, status
                );
                match stop {
                    Some(false) => self.state = AppState::Stopped,
                    _ => self.state = AppState::Idle,
                };
                true
            }
            AppState::Stopping {
                mut service_pid,
                mut pid,
                restart,
            }
                if service_pid == Some(child) || pid == Some(child) =>
            {
                if service_pid == Some(child) {
                    service_pid = None;
                }
                if pid == Some(child) {
                    pid = None
                }
                if service_pid.is_some() || pid.is_some() {
                    self.state = AppState::Stopping {
                        service_pid,
                        pid,
                        restart,
                    };
                } else if restart {
                    self.state = AppState::Idle;
                } else {
                    self.state = AppState::Stopped;
                }
                true
            }
            _ => false,
        }
    }

    pub fn is_scheduled_stop(&self) -> bool {
        match self.state {
            AppState::Running { stop: Some(_), .. } => true,
            _ => false,
        }
    }

    pub fn is_stopping(&self) -> bool {
        match self.state {
            AppState::Stopping { .. } => true,
            _ => false,
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.state == AppState::Stopped
    }

    pub fn is_idle(&self) -> bool {
        self.state == AppState::Idle
    }

    pub fn is_running(&self) -> bool {
        match self.state {
            AppState::Running { pid: Some(_), .. } => true,
            _ => false,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.mode == Mode::OneShot && self.state == AppState::Stopped
    }

    pub fn get_service_pid(&self) -> Option<u32> {
        match self.state {
            AppState::Running { pid, .. } => pid,
            AppState::Stopping { pid, .. } => pid,
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

    pub fn read_pidfile(&self) -> Option<u32> {
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
