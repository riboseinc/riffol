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

use crossbeam_channel as cc;
use rand::{thread_rng, Rng};
use std::ffi::CString;
use std::fs::{read_dir, File};
use std::io::Read;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub fn recv_checks(checks: &[IntervalHealthCheck]) -> cc::Receiver<(String, String)> {
    let (fail_send, fail_recv) = cc::unbounded();
    checks.iter().for_each(|check| {
        let fail_tx = fail_send.clone();
        let group = check.group.to_owned();
        let check = check.clone();
        thread::spawn(move || {
            // make the first check happen at random point between now and now + check.interval
            let mut next =
                Instant::now()
                    + Duration::from_secs(thread_rng().gen_range(0, check.interval.as_secs()));
            let message = check.to_string();
            loop {
                thread::sleep(next - (Instant::now().min(next)));
                next += check.interval;
                debug!("Healthcheck: {}", message);
                check
                    .do_check()
                    .map_err(|e| {
                        debug!("Healthcheck failed: {} [{}].", message, e);
                        fail_tx.send((group.to_owned(), message.to_owned()));
                    }).ok();
            }
        });
    });
    fail_recv
}

#[derive(Debug, Clone)]
pub enum HealthCheck {
    DfCheck(DfCheck),
    ProcCheck(ProcCheck),
    TcpCheck(TcpCheck),
}

impl HealthCheck {
    pub fn do_check(&self) -> Result<(), String> {
        match self {
            HealthCheck::DfCheck(s) => s.do_check(),
            HealthCheck::ProcCheck(s) => s.do_check(),
            HealthCheck::TcpCheck(s) => s.do_check(),
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            HealthCheck::DfCheck(s) => s.to_string(),
            HealthCheck::ProcCheck(s) => s.to_string(),
            HealthCheck::TcpCheck(s) => s.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IntervalHealthCheck {
    pub group: String,
    pub interval: Duration,
    pub timeout: Duration,
    pub check: HealthCheck,
}

impl IntervalHealthCheck {
    pub fn to_string(&self) -> String {
        self.check.to_string()
    }

    pub fn do_check(&self) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        let check = self.check.clone();
        thread::spawn(move || {
            let _t = tx.send(check.do_check());
        });
        rx.recv_timeout(self.timeout)
            .map_err(|_| "Timeout".to_owned())
            .and_then(|res| res)
    }
}

#[derive(Debug, Clone)]
pub struct DfCheck {
    free: u64,
    path: CString,
}

impl DfCheck {
    pub fn new(path: &Path, free: u64) -> DfCheck {
        DfCheck {
            path: CString::new(path.to_string_lossy().into_owned()).unwrap(),
            free,
        }
    }

    fn to_string(&self) -> String {
        format!("DF healthcheck, min {}MB for {:?}", self.free, self.path)
    }

    fn do_check(&self) -> Result<(), String> {
        #[cfg(statvfs64)]
        // use statvfs to get blocks available to unpriviliged users
        let free = unsafe {
            use libc::statvfs64;
            let mut stats: statvfs64 = ::std::mem::uninitialized();
            if statvfs64(self.path.as_ptr(), &mut stats) == 0 {
                Ok(stats.f_bsize as u64 * stats.f_bavail / 1024 / 1024)
            } else {
                Err("Couldn't read".to_owned())
                //Err(libc::strerror(*__errno_location()))
            }
        };

        #[cfg(not(statvfs64))]
        // use statvfs to get blocks available to unpriviliged users
        let free = unsafe {
            use libc::statvfs;
            let mut stats: statvfs = ::std::mem::uninitialized();
            if statvfs(self.path.as_ptr(), &mut stats) == 0 {
                Ok(stats.f_bsize as u64 * (stats.f_bavail / 1024 / 1024) as u64)
            } else {
                Err("Couldn't read".to_owned())
                //Err(libc::strerror(*__errno_location()))
            }
        };

        free.and_then(|f| {
            if f > self.free {
                Ok(())
            } else {
                Err(format!(
                    "Insufficient disk space. {}MB available, {}MB required",
                    f, self.free
                ))
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct ProcCheck {
    process: String,
}

impl ProcCheck {
    pub fn new(process: &str) -> ProcCheck {
        ProcCheck {
            process: String::from(process),
        }
    }

    fn to_string(&self) -> String {
        format!("Proc healthcheck, checking for {}", self.process)
    }

    // match against /proc/pid/comm
    // zombie if /proc/pid/cmdline is empty
    fn do_check(&self) -> Result<(), String> {
        read_dir("/proc")
            .map_err(|_| "Couldn't read procfs".to_owned())
            .and_then(|mut it| {
                it.find(|ref result| {
                    result
                        .as_ref()
                        .ok()
                        .and_then(|entry| entry.file_name().to_string_lossy().parse::<u32>().ok())
                        .filter(|pid| {
                            let mut contents = String::new();
                            File::open(format!("/proc/{}/comm", pid))
                                .and_then(|mut f| f.read_to_string(&mut contents))
                                .map(|_| {
                                    contents.pop();
                                    contents == self.process
                                }).unwrap_or(false)
                        }).map(|pid| {
                            File::open(format!("/proc/{}/cmdline", pid))
                                .ok()
                                .map_or_else(|| false, |f| f.bytes().next().is_some())
                        }).unwrap_or(false)
                }).ok_or_else(|| "No such process".to_owned())
                .map(|_| ())
            })
    }
}

#[derive(Debug, Clone)]
pub struct TcpCheck {
    addr: SocketAddr,
}

impl TcpCheck {
    pub fn new(addr: &SocketAddr) -> TcpCheck {
        TcpCheck { addr: (*addr) }
    }

    fn to_string(&self) -> String {
        format!("TCP healthcheck, connect to {:?}", self.addr)
    }

    fn do_check(&self) -> Result<(), String> {
        TcpStream::connect(&self.addr)
            .map(|_| ())
            .map_err(|e| format!("Failed ({})", e))
    }
}
