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

use limit::{setlimit, RLimit};
use std::collections::HashMap;
use std::io;
use std::os::unix::io::IntoRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use stream;

#[derive(Debug, PartialEq)]
pub enum AppAction {
    Restart,
}

impl FromStr for AppAction {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "restart" => Ok(AppAction::Restart),
            _ => Err(format!("No such AppAction \"{}\"", s)),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Mode {
    Simple,
    Forking,
    OneShot,
}

#[derive(Debug)]
pub struct Application {
    pub mode: Mode,
    pub exec: String,
    pub dir: String,
    pub env: HashMap<String, String>,
    pub start: String,
    pub stop: String,
    pub restart: String,
    pub healthchecks: Vec<String>,
    pub healthcheckfail: AppAction,
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
        fds: (Option<i32>, Option<i32>, Option<i32>),
        stop: Option<bool>,
    },
    Running {
        stop: Option<bool>,
        pid: Option<u32>,
    },
    Stopping {
        pid: u32,
        restart: bool,
    },
    Stopped,
}

impl Application {
    pub fn start(&mut self) -> io::Result<()> {
        self.start_process(&self.start).map(|mut child| {
            self.state = AppState::Starting {
                pid: child.id(),
                fds: (
                    child.stdin.take().map(|s| s.into_raw_fd()),
                    child.stdout.take().map(|s| s.into_raw_fd()),
                    child.stderr.take().map(|s| s.into_raw_fd()),
                ),
                stop: None,
            }
        })
    }

    pub fn stop(&mut self, restart: bool) -> io::Result<()> {
        self.start_process(&self.stop).map(|child| {
            self.state = AppState::Stopping {
                pid: child.id(),
                restart,
            }
        })
    }

    fn start_process(&self, arg: &str) -> io::Result<Child> {
        fn stdio(stream: &Option<stream::Stream>) -> Stdio {
            stream
                .as_ref()
                .map(|stream| match stream {
                    stream::Stream::File { filename: f } if f == "/dev/null" => Stdio::null(),
                    _ => Stdio::piped(),
                }).unwrap_or_else(Stdio::inherit)
        }

        let limits = self.limits.clone();

        Command::new(&self.exec)
            .current_dir(&self.dir)
            .env_clear()
            .envs(self.env.iter())
            .before_exec(move || {
                limits.iter().for_each(|l| setlimit(l));
                Ok(())
            }).stdout(stdio(&self.stdout))
            .stderr(stdio(&self.stderr))
            .arg(arg)
            .spawn()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_app_action() {
        use super::AppAction;
        assert_eq!("restart".parse(), Ok(AppAction::Restart));
        assert!("restrt".parse::<AppAction>().is_err());
    }
}
