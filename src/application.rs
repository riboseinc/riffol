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

extern crate syslog;

use self::syslog::{Formatter3164, Logger, LoggerBackend, Severity::*};
use health::IntervalHealthCheck;
use limit::{setlimit, RLimit};
use std::collections::HashMap;
use std::env;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, LineWriter, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;
use stream;

#[derive(Debug, Deserialize, Clone)]
#[serde(field_identifier, rename_all = "lowercase")]
pub enum AppAction {
    Restart,
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
    pub fn start(&mut self) -> bool {
        let limits = self.limits.clone();
        match Command::new(&self.exec)
            .arg(&self.start)
            .current_dir(&self.dir)
            .env_clear()
            .envs(self.env.iter())
            .before_exec(move || {
                limits.iter().for_each(|l| setlimit(l));
                Ok(())
            })
            .stdout(stdio(&self.stdout))
            .stderr(stdio(&self.stderr))
            .spawn()
        {
            Ok(mut child) => {
                if let Some(s) = child.stdout.as_ref() {
                    self.stdout_thread = spawn_stream_thread(&self.stdout, s.as_raw_fd());
                }
                if let Some(s) = child.stderr.as_ref() {
                    self.stderr_thread = spawn_stream_thread(&self.stderr, s.as_raw_fd());
                }
                match child.wait() {
                    Ok(ref s) if s.success() => {
                        log(format!("Successfully spawned {}", self.exec));
                        self.state = AppState::Running;
                        true
                    }
                    _ => {
                        log(format!("Application exited with error {}", self.exec));
                        self.state = AppState::Failed;
                        false
                    }
                }
            }
            Err(_) => {
                log(format!("Failed to spawn {}", self.exec));
                self.state = AppState::Failed;
                false
            }
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
            })
            .stdout(stdio(&self.stdout))
            .stderr(stdio(&self.stderr))
            .spawn()
            .and_then(|mut c| c.wait());
    }

    pub fn spawn_check_threads<T: Send + Sync + Clone + 'static>(
        &mut self,
        fail_tx: mpsc::Sender<Option<T>>,
        fail_msg: T,
    ) -> () {
        self.check_threads = self.healthchecks
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
                                log(format!("Cancelling {}.", check.to_string()));
                                break;
                            }
                            // otherwise we timeout so proceed with the check
                            _ => {
                                next += check.interval;
                                log(format!("{}.", check.to_string()));
                                match check.do_check() {
                                    Ok(_) => (),
                                    Err(e) => {
                                        log(format!("{}. {}.", check.to_string(), e));
                                        let _t = fail_tx.send(Some(fail_msg.clone()));
                                    }
                                }
                            }
                        }
                    }
                });
                (tx, h)
            })
            .collect();
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

fn spawn_stream_thread(
    stream: &Option<stream::Stream>,
    source: RawFd,
) -> Option<thread::JoinHandle<()>> {
    let source = BufReader::new(unsafe { File::from_raw_fd(source) });
    match stream {
        Some(stream) => match stream {
            stream::Stream::File { filename } => {
                let f = filename.to_owned();
                Some(thread::spawn(move || {
                    let mut sink =
                        LineWriter::new(OpenOptions::new().append(true).open(f).unwrap());

                    for l in source.lines() {
                        sink.write(l.as_ref().unwrap().as_bytes()).unwrap();
                    }
                }))
            }
            stream::Stream::Syslog {
                address,
                facility,
                severity,
            } => {
                let formatter = Formatter3164 {
                    facility: facility.clone(),
                    hostname: None,
                    process: String::from("riffol"),
                    pid: 0,
                };
                let mut logger: syslog::Result<
                    Logger<LoggerBackend, String, Formatter3164>,
                > = match address {
                    stream::Address::Unix(address) => match address {
                        Some(address) => syslog::unix_custom(formatter, address),
                        None => syslog::unix(formatter),
                    },
                    stream::Address::Tcp(server) => syslog::tcp(formatter, server),
                    stream::Address::Udp { server, local } => syslog::udp(formatter, local, server),
                };

                match logger {
                    Ok(mut logger) => {
                        let severity = severity.clone();
                        Some(thread::spawn(move || {
                            for l in source.lines() {
                                let l = l.unwrap();
                                match severity {
                                    x if x == LOG_EMERG as u32 => logger.emerg(l),
                                    x if x == LOG_ALERT as u32 => logger.alert(l),
                                    x if x == LOG_CRIT as u32 => logger.crit(l),
                                    x if x == LOG_ERR as u32 => logger.err(l),
                                    x if x == LOG_WARNING as u32 => logger.warning(l),
                                    x if x == LOG_NOTICE as u32 => logger.notice(l),
                                    x if x == LOG_INFO as u32 => logger.info(l),
                                    _ => logger.debug(l),
                                }.unwrap();
                            }
                        }))
                    }
                    Err(_) => None,
                }
            }
        },
        _ => None,
    }
}

fn log(s: String) {
    let arg0 = env::args().next().unwrap();
    eprintln!("{}: {}", arg0, s);
}
