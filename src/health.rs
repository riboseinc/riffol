use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug)]
pub enum HealthCheck {
    DfCheck(DfCheck),
    ProcCheck(ProcCheck),
    TcpCheck(TcpCheck),
}

impl HealthCheck {
    pub fn do_check(&self) -> bool {
        match self {
            HealthCheck::DfCheck(s) => s.do_check(),
            HealthCheck::ProcCheck(s) => s.do_check(),
            HealthCheck::TcpCheck(s) => s.do_check(),
        }
    }
}

#[derive(Debug)]
pub struct IntervalHealthCheck {
    pub interval: Duration,
    pub timeout: Duration,
    the_check: HealthCheck,
}

impl IntervalHealthCheck {
    pub fn new(interval: Duration, timeout: Duration, check: HealthCheck) -> IntervalHealthCheck {
        IntervalHealthCheck {
            interval: interval,
            timeout: timeout,
            the_check: check,
        }
    }

    pub fn do_check(&self) -> bool {
        self.the_check.do_check()
    }
}

#[derive(Debug)]
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

#[derive(Debug)]
pub struct ProcCheck {
    process: String,
}

impl ProcCheck {
    pub fn new(process: &str) -> ProcCheck {
        ProcCheck {
            process: String::from(process),
        }
    }

    fn do_check(&self) -> bool {
        match Command::new("/bin/pidof").arg(&self.process).status() {
            Ok(s) if s.success() => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct TcpCheck {
    addr: SocketAddr,
}

impl TcpCheck {
    pub fn new(addr: &SocketAddr) -> TcpCheck {
        TcpCheck {
            addr: (*addr).clone(),
        }
    }

    fn do_check(&self) -> bool {
        match TcpStream::connect(&self.addr) {
            Ok(_) => true,
            Err(_) => false,
        }
    }
}
