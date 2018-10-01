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

extern crate libc;

use health::IntervalHealthCheck;
use limit::{setlimit, RLimit};
use std::collections::HashMap;
use std::io;
use std::os::unix::io::IntoRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;
use stream;

#[derive(Debug, PartialEq)]
pub enum AppAction {
    Restart,
}

impl FromStr for AppAction {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.as_ref() {
            "restart" => Ok(AppAction::Restart),
            _ => Err(format!("No such AppAction \"{}\"", s)),
        }
    }
}

#[derive(Debug)]
pub struct Application {
    pub exec: String,
    pub dir: String,
    pub env: HashMap<String, String>,
    pub start: String,
    pub stop: String,
    pub restart: String,
    pub healthchecks: Vec<IntervalHealthCheck>,
    pub healthcheckfail: AppAction,
    pub limits: Vec<RLimit>,
    pub stdout: Option<stream::Stream>,
    pub stderr: Option<stream::Stream>,
    pub state: AppState,
    pub check_threads: Vec<(mpsc::Sender<()>, thread::JoinHandle<()>)>,
    pub stdout_thread: Option<thread::JoinHandle<()>>,
    pub stderr_thread: Option<thread::JoinHandle<()>>,
}

#[derive(Debug, PartialEq)]
pub enum AppState {
    Running,
    Failed,
    Stopped,
}

impl Application {
    pub fn start(&mut self, stream_handler: &stream::Handler) -> io::Result<()> {
        let limits = self.limits.clone();
        self.state = AppState::Failed;

        let mut child = Command::new(&self.exec)
            .arg(&self.start)
            .current_dir(&self.dir)
            .env_clear()
            .envs(self.env.iter())
            .before_exec(move || {
                limits.iter().for_each(|l| setlimit(l));
                Ok(())
            }).stdout(stdio(&self.stdout))
            .stderr(stdio(&self.stderr))
            .spawn()?;

        if let Some(stdout) = child.stdout.take() {
            if let Err(e) =
                stream_handler.add_stream(stdout.into_raw_fd(), self.stdout.as_ref().unwrap())
            {
                warn!("Failed to capture stdout for {} ({})", self.exec, e);
            }
        }
        if let Some(stderr) = child.stderr.take() {
            if let Err(e) =
                stream_handler.add_stream(stderr.into_raw_fd(), self.stderr.as_ref().unwrap())
            {
                warn!("Failed to capture stderr for {} ({})", self.exec, e);
            }
        }
        let status = child.wait()?;

        match status.success() {
            true => {
                self.state = AppState::Running;
                Ok(())
            }
            false => Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Abnormal exit status ({})", status),
            )),
        }
    }

    pub fn stop(&mut self) {
        let _result = Command::new(&self.exec)
            .arg(&self.stop)
            .current_dir(&self.dir)
            .env_clear()
            .envs(self.env.iter())
            .spawn()
            .and_then(|mut c| c.wait());
        self.state = AppState::Stopped;
    }

    pub fn restart(&self) {
        let limits = self.limits.to_owned();
        let _result = Command::new(&self.exec)
            .arg(&self.restart)
            .current_dir(&self.dir)
            .env_clear()
            .envs(self.env.iter())
            .before_exec(move || {
                limits.iter().for_each(|l| setlimit(l));
                Ok(())
            }).stdout(stdio(&self.stdout))
            .stderr(stdio(&self.stderr))
            .spawn()
            .and_then(|mut c| c.wait());
    }

    pub fn spawn_check_threads<T: Send + Sync + Clone + 'static>(
        &mut self,
        fail_tx: mpsc::Sender<Option<T>>,
        fail_msg: T,
    ) -> () {
        self.check_threads = self
            .healthchecks
            .iter()
            .map(|c| {
                let check = c.clone();
                let fail_tx = fail_tx.clone();
                let fail_msg = fail_msg.clone();
                let (tx, rx) = mpsc::channel();
                let h = thread::spawn(move || {
                    let mut next = Instant::now() + check.interval;
                    loop {
                        match rx.recv_timeout(next - Instant::now()) {
                            // Ok(()) means we received the 'kill' message
                            Ok(()) => {
                                info!("Cancelling {}.", check.to_string());
                                break;
                            }
                            // otherwise we timeout so proceed with the check
                            _ => {
                                next += check.interval;
                                debug!("{}.", check.to_string());
                                match check.do_check() {
                                    Ok(_) => (),
                                    Err(e) => {
                                        warn!("{}. {}.", check.to_string(), e);
                                        let _t = fail_tx.send(Some(fail_msg.clone()));
                                    }
                                }
                            }
                        }
                    }
                });
                (tx, h)
            }).collect();
    }

    pub fn stop_check_threads(&mut self) {
        for (tx, h) in self.check_threads.drain(..) {
            let _t = tx.send(());
            let _t = h.join();
        }
    }
}

fn stdio(stream: &Option<stream::Stream>) -> Stdio {
    match stream {
        Some(stream) => match stream {
            stream::Stream::File { filename: f } => {
                if f == "/dev/null" {
                    Stdio::null()
                } else {
                    Stdio::piped()
                }
            }
            _ => Stdio::piped(),
        },
        None => Stdio::inherit(),
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
