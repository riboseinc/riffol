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

use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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
    pub interval: Duration,
    pub timeout: Duration,
    pub the_check: HealthCheck,
}

impl IntervalHealthCheck {
    pub fn new(interval: Duration, timeout: Duration, check: HealthCheck) -> IntervalHealthCheck {
        IntervalHealthCheck {
            interval,
            timeout,
            the_check: check,
        }
    }

    pub fn to_string(&self) -> String {
        self.the_check.to_string()
    }

    pub fn do_check(&self) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        let check = self.the_check.clone();
        thread::spawn(move || {
            let _t = tx.send(check.do_check());
        });
        match rx.recv_timeout(self.timeout) {
            Ok(res) => res,
            Err(_) => Err("Timeout".to_owned()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DfCheck {
    free: u64,
    path: PathBuf,
}

impl DfCheck {
    pub fn new(path: &Path, free: u64) -> DfCheck {
        DfCheck {
            path: PathBuf::from(path),
            free,
        }
    }

    fn to_string(&self) -> String {
        format!("DF healthcheck, min {}MB for {:?}", self.free, self.path)
    }

    fn do_check(&self) -> Result<(), String> {
        fn avail(o: &[u8]) -> Option<u64> {
            match String::from_utf8_lossy(o).lines().nth(1) {
                Some(s) => match s.trim_right_matches('M').parse::<u64>() {
                    Ok(n) => Some(n),
                    Err(_) => None,
                },
                None => None,
            }
        };

        let fail = "Failed to get free space";
        match Command::new("/bin/df")
            .arg("-BM")
            .arg("--output=avail")
            .arg(&self.path)
            .stderr(Stdio::null())
            .output()
        {
            Ok(o) => match (o.status.success(), avail(&o.stdout)) {
                (true, Some(n)) if n >= self.free => Ok(()),
                (true, Some(n)) => Err(format!("{}MB free", n,)),
                _ => Err(fail.to_owned()),
            },
            _ => Err(fail.to_owned()),
        }
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

    fn do_check(&self) -> Result<(), String> {
        match Command::new("pidof")
            .arg(&self.process)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(s) if s.success() => Ok(()),
            _ => Err("No such process".to_owned()),
        }
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
        match TcpStream::connect(&self.addr) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed ({})", e)),
        }
    }
}
