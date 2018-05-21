use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub trait HealthCheck {
    fn do_check(&self) -> bool;
}

pub struct IntervalHealthCheck {
    interval: Duration,
    the_check: Box<HealthCheck>,
}

impl IntervalHealthCheck {
    pub fn new(interval: Duration, check: Box<HealthCheck>) -> IntervalHealthCheck {
        IntervalHealthCheck {
            interval: interval,
            the_check: check,
        }
    }
    pub fn get_interval(&self) -> &Duration {
        &self.interval
    }
}

impl HealthCheck for IntervalHealthCheck {
    fn do_check(&self) -> bool {
        self.the_check.do_check()
    }
}

pub struct DfCheck {
    free: u64,
    path: PathBuf,
}

impl DfCheck {
    pub fn new(path: &Path, free: u64) -> DfCheck {
        DfCheck {
            path: PathBuf::from(path),
            free: free,
        }
    }
}

impl HealthCheck for DfCheck {
    fn do_check(&self) -> bool {
        fn avail(o: &Vec<u8>) -> Option<u64> {
            match String::from_utf8_lossy(o).lines().skip(1).next() {
                Some(s) => match s.trim_right_matches('M').parse::<u64>() {
                    Ok(n) => Some(n),
                    Err(_) => None,
                },
                None => None,
            }
        };

        match Command::new("/bin/df")
            .arg("-BM")
            .arg("--output=avail")
            .arg(&self.path)
            .output()
        {
            Ok(o) => match (o.status.success(), avail(&o.stdout)) {
                (true, Some(n)) => self.free < n,
                _ => false,
            },
            _ => false,
        }
    }
}

pub struct ProcCheck {
    process: String,
}

impl ProcCheck {
    pub fn new(process: &str) -> ProcCheck {
        ProcCheck {
            process: String::from(process),
        }
    }
}

impl HealthCheck for ProcCheck {
    fn do_check(&self) -> bool {
        match Command::new("/bin/pidof").arg(&self.process).status() {
            Ok(s) if s.success() => true,
            _ => false,
        }
    }
}

pub struct TcpCheck {
    addr: SocketAddr,
    timeout: Duration,
}

impl TcpCheck {
    pub fn new(addr: &SocketAddr, timeout: &Duration) -> TcpCheck {
        TcpCheck {
            addr: (*addr).clone(),
            timeout: (*timeout).clone(),
        }
    }
}

impl HealthCheck for TcpCheck {
    fn do_check(&self) -> bool {
        match TcpStream::connect_timeout(&self.addr, self.timeout) {
            Ok(_) => true,
            Err(_) => false,
        }
    }
}
