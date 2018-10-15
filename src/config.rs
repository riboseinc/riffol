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

use application::{self, AppState, Mode};
use health::{DfCheck, HealthCheck, IntervalHealthCheck, ProcCheck, TcpCheck};
use limit::{Limit, RLimit};
use nereon::{self, FromValue, Value};
use std::collections::HashMap;
use std::iter::Iterator;
use std::net::SocketAddr;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use std::{env, fs};
use stream;
use syslog;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
const LICENSE: &str = "BSD-2-Clause";
const APPNAME: &str = env!("CARGO_PKG_NAME");

#[derive(FromValue)]
struct Config {
    init: HashMap<String, Init>,
    application_group: HashMap<String, AppGroup>,
    application: HashMap<String, Application>,
    dependency: HashMap<String, Dependencies>,
    healthchecks: HashMap<String, HealthChecks>,
    limits: HashMap<String, Limits>,
}

#[derive(FromValue)]
struct Init {
    application_groups: Vec<String>,
}

#[derive(FromValue)]
struct AppGroup {
    applications: Vec<String>,
    dependencies: Vec<String>,
}

#[derive(FromValue)]
struct Environment {
    pass: HashMap<String, String>,
    new: HashMap<String, String>,
}

#[derive(FromValue)]
struct Application {
    mode: Option<String>,
    dir: Option<String>,
    pidfile: Option<String>,
    env: Option<Environment>,
    env_file: Option<String>,
    start: Vec<String>,
    stop: Vec<String>,
    healthchecks: Vec<String>,
    limits: Vec<String>,
    stdout: Option<Stream>,
    stderr: Option<Stream>,
    requires: Vec<String>,
}

#[derive(FromValue)]
struct Dependencies {
    packages: Vec<String>,
}

type Limits = HashMap<String, u64>;

#[derive(FromValue)]
struct HealthChecks {
    checks: Vec<String>,
    timeout: u64,
    interval: u64,
}

#[derive(FromValue)]
enum SyslogSeverity {
    EMERG,
    ALERT,
    CRIT,
    ERR,
    WARNING,
    NOTICE,
    INFO,
    DEBUG,
}

#[derive(FromValue)]
enum SyslogFacility {
    KERN,
    USER,
    MAIL,
    DAEMON,
    AUTH,
    SYSLOG,
    LPR,
    NEWS,
    UUCP,
    CRON,
    AUTHPRIV,
    FTP,
    LOCAL0,
    LOCAL1,
    LOCAL2,
    LOCAL3,
    LOCAL4,
    LOCAL5,
    LOCAL6,
    LOCAL7,
}

#[derive(FromValue)]
enum Stream {
    File(String),
    Syslog {
        socket: Option<String>,
        facility: Option<SyslogFacility>,
        severity: Option<SyslogSeverity>,
    },
    RSyslog {
        server: String,
        local: Option<String>,
        facility: Option<SyslogFacility>,
        severity: Option<SyslogSeverity>,
    },
}

#[derive(Debug)]
pub struct Riffol {
    pub applications: Vec<application::Application>,
    pub dependencies: Vec<String>,
    pub healthchecks: Vec<IntervalHealthCheck>,
}

pub fn get_config<T: IntoIterator<Item = String>>(args: T) -> Result<Riffol, String> {
    let nos = format!(
        r#"
        authors ["{}"]
        license "{}"
        name "{}"
        version {}
        option config {{
            flags [takesvalue, required]
            short f
            long file
            default "/etc/riffol.conf"
            env RIFFOL_CONFIG
            hint FILE
            usage "Configuration file"
        }}"#,
        AUTHORS, LICENSE, APPNAME, VERSION
    );

    let config = nereon::configure::<Config, _, _>(&nos, args)?;

    let mut riffol = Riffol {
        applications: Vec::new(),
        dependencies: Vec::new(),
        healthchecks: Vec::new(),
    };

    for (_, init) in config.init {
        for group_name in init.application_groups {
            match config.application_group.get(&group_name) {
                Some(group) => {
                    for id in &group.applications {
                        match config.application.get(id) {
                            Some(ap) => {
                                let mode = ap.mode.as_ref().map_or_else(
                                    || Ok(Mode::Simple),
                                    |s| match s.as_ref() {
                                        "simple" => Ok(Mode::Simple),
                                        "forking" => Ok(Mode::Forking),
                                        "oneshot" => Ok(Mode::OneShot),
                                        _ => Err(format!("Invalid application mode ({})", s)),
                                    },
                                )?;

                                let healthchecks = ap.healthchecks.clone();
                                let limits = match get_limits(&config.limits, &ap.limits) {
                                    Ok(ls) => ls,
                                    Err(e) => return Err(e),
                                };
                                let mut env = ap
                                    .env_file
                                    .as_ref()
                                    .map_or_else(|| Ok(HashMap::new()), |f| read_env_file(&f))?;

                                if let Some(ref vars) = ap.env {
                                    vars.pass.iter().for_each(|(old, new)| {
                                        if let Ok(value) = env::var(old) {
                                            env.insert(new.to_owned(), value.to_owned());
                                        }
                                    });
                                    vars.new.iter().for_each(|(k, v)| {
                                        env.insert(k.to_owned(), v.to_owned());
                                    });
                                }

                                let stderr = match ap.stderr.as_ref() {
                                    None => None,
                                    Some(s) => match mk_stream(&s) {
                                        Ok(s) => Some(s),
                                        Err(e) => return Err(format!("Invalid stream {}", e)),
                                    },
                                };

                                let stdout = match ap.stdout.as_ref() {
                                    None => None,
                                    Some(s) => match mk_stream(&s) {
                                        Ok(s) => Some(s),
                                        Err(e) => return Err(format!("Invalid stream {}", e)),
                                    },
                                };

                                riffol.applications.push(application::Application {
                                    id: id.to_owned(),
                                    mode,
                                    dir: ap.dir.clone().unwrap_or_else(|| "/tmp".to_owned()),
                                    pidfile: ap.pidfile.clone(),
                                    env,
                                    start: ap.start.clone(),
                                    stop: ap.stop.clone(),
                                    healthchecks,
                                    limits,
                                    stdout,
                                    stderr,
                                    depends: ap.requires.iter().map(|r| r.to_owned()).collect(),
                                    state: AppState::Idle,
                                });
                            }
                            None => return Err(format!("No such application \"{}\"", id)),
                        }
                    }
                    for dep_name in &group.dependencies {
                        match config.dependency.get(dep_name) {
                            Some(dep) => dep
                                .packages
                                .iter()
                                .for_each(|d| riffol.dependencies.push(d.clone())),
                            None => return Err(format!("No such dependencies \"{}\"", dep_name)),
                        }
                    }
                }
                None => return Err(format!("No such application_group \"{}\"", group_name)),
            }
        }
    }
    riffol.healthchecks =
        config
            .healthchecks
            .iter()
            .try_fold(Vec::new(), |checks, (group, check)| {
                check.checks.iter().try_fold(checks, |mut checks, params| {
                    mk_interval_healthcheck(group, check.interval, check.timeout, params).map(
                        |check| {
                            checks.push(check);
                            checks
                        },
                    )
                })
            })?;

    Ok(riffol)
}

fn read_env_file(filename: &str) -> Result<HashMap<String, String>, String> {
    fs::read_to_string(filename)
        .map_err(|e| format!("Cant't read env_file {}: {:?}", filename, e))
        .map(|s| {
            s.lines()
                .map(|v| {
                    let kv = v.splitn(2, '=').collect::<Vec<&str>>();
                    match kv.len() {
                        1 => (kv[0].to_owned(), "".to_owned()),
                        _ => (kv[0].to_owned(), kv[1].to_owned()),
                    }
                }).collect()
        })
}

fn mk_interval_healthcheck(
    group: &str,
    interval: u64,
    timeout: u64,
    check: &str,
) -> Result<IntervalHealthCheck, String> {
    Ok(IntervalHealthCheck {
        group: group.to_owned(),
        interval: Duration::from_secs(interval),
        timeout: Duration::from_secs(timeout),
        check: mk_healthcheck(check)?,
    })
}

fn mk_healthcheck(params: &str) -> Result<HealthCheck, String> {
    let split2 = |p, s: &str| {
        let svec: Vec<&str> = s.splitn(2, p).collect();
        match (svec.get(0), svec.get(1)) {
            (Some(&s1), Some(&s2)) => (s1.to_owned(), s2.to_owned()),
            _ => (s.to_owned(), "".to_owned()),
        }
    };

    let (check, args) = split2("://", params);
    let bad = |u| Err(format!("Bad {0} healthcheck. Use \"{0}//{1}\"", check, u));
    match check.as_ref() {
        "proc" => match args {
            ref s if !s.is_empty() => Ok(HealthCheck::ProcCheck(ProcCheck::new(&args))),
            _ => bad("<process>"),
        },
        "df" => {
            let bad_df = || bad("<file>:<free>");
            match split2(":", &args) {
                (ref file, ref free) if !file.is_empty() => match free.parse() {
                    Ok(n) => Ok(HealthCheck::DfCheck(DfCheck::new(Path::new(&file), n))),
                    _ => bad_df(),
                },
                _ => bad_df(),
            }
        }
        "tcp" => match args.parse() {
            Ok(addr) => Ok(HealthCheck::TcpCheck(TcpCheck::new(&addr))),
            _ => bad("<ip-address>"),
        },
        p => Err(format!("Unknown healthcheck type {}", p)),
    }
}

fn get_limits(configs: &HashMap<String, Limits>, limits: &[String]) -> Result<Vec<RLimit>, String> {
    let min = |a, b| match (a, b) {
        (x, Limit::Infinity) => x,
        (Limit::Num(x), Limit::Num(y)) if x < y => Limit::Num(x),
        (_, y) => y,
    };

    let mut procs = Limit::Infinity;
    let mut mem = Limit::Infinity;
    let mut files = Limit::Infinity;

    for name in limits.iter() {
        match configs.get(name) {
            Some(config) => {
                for (k, v) in config.iter() {
                    match k.as_ref() {
                        "max_procs" => procs = min(procs, Limit::Num(*v)),
                        "max_mem" => mem = min(mem, Limit::Num(*v * 1024 * 1024)),
                        "max_files" => files = min(files, Limit::Num(*v)),
                        _ => return Err(format!("No such limit ({}).", k)),
                    }
                }
            }
            None => return Err(format!("No such limits \"{}\"", name)),
        }
    }
    Ok(vec![
        RLimit::Procs(procs),
        RLimit::Memory(mem),
        RLimit::Files(files),
    ])
}

fn mk_stream(stream: &Stream) -> Result<stream::Stream, String> {
    match stream {
        Stream::File(filename) => Ok(stream::Stream::File {
            filename: filename.to_owned(),
        }),
        Stream::Syslog {
            socket,
            facility,
            severity,
        } => Ok(stream::Stream::Syslog {
            address: stream::Address::Unix(socket.to_owned()),
            facility: config_to_syslog_facility(facility),
            severity: config_to_syslog_severity(severity),
        }),
        Stream::RSyslog {
            server,
            local,
            facility: f,
            severity: s,
        } => Ok(stream::Stream::Syslog {
            address: {
                if let Ok(server) = SocketAddr::from_str(server) {
                    match local {
                        Some(local) => match SocketAddr::from_str(local) {
                            Ok(local) => stream::Address::Udp { server, local },
                            Err(_) => return Err(format!("Not a valid inet address: {}", local)),
                        },
                        None => stream::Address::Tcp(server),
                    }
                } else {
                    return Err(format!("Not a valid inet address: {}", server));
                }
            },
            facility: config_to_syslog_facility(f),
            severity: config_to_syslog_severity(s),
        }),
    }
}

fn config_to_syslog_facility(f: &Option<SyslogFacility>) -> syslog::Facility {
    f.as_ref()
        .map(|f| match f {
            SyslogFacility::KERN => syslog::Facility::LOG_KERN,
            SyslogFacility::USER => syslog::Facility::LOG_USER,
            SyslogFacility::MAIL => syslog::Facility::LOG_MAIL,
            SyslogFacility::DAEMON => syslog::Facility::LOG_DAEMON,
            SyslogFacility::AUTH => syslog::Facility::LOG_AUTH,
            SyslogFacility::SYSLOG => syslog::Facility::LOG_SYSLOG,
            SyslogFacility::LPR => syslog::Facility::LOG_LPR,
            SyslogFacility::NEWS => syslog::Facility::LOG_NEWS,
            SyslogFacility::UUCP => syslog::Facility::LOG_UUCP,
            SyslogFacility::CRON => syslog::Facility::LOG_CRON,
            SyslogFacility::AUTHPRIV => syslog::Facility::LOG_AUTHPRIV,
            SyslogFacility::FTP => syslog::Facility::LOG_FTP,
            SyslogFacility::LOCAL0 => syslog::Facility::LOG_LOCAL0,
            SyslogFacility::LOCAL1 => syslog::Facility::LOG_LOCAL1,
            SyslogFacility::LOCAL2 => syslog::Facility::LOG_LOCAL2,
            SyslogFacility::LOCAL3 => syslog::Facility::LOG_LOCAL3,
            SyslogFacility::LOCAL4 => syslog::Facility::LOG_LOCAL4,
            SyslogFacility::LOCAL5 => syslog::Facility::LOG_LOCAL5,
            SyslogFacility::LOCAL6 => syslog::Facility::LOG_LOCAL6,
            SyslogFacility::LOCAL7 => syslog::Facility::LOG_LOCAL7,
        }).unwrap_or(syslog::Facility::LOG_DAEMON)
}

fn config_to_syslog_severity(s: &Option<SyslogSeverity>) -> u32 {
    s.as_ref()
        .map(|s| match s {
            SyslogSeverity::EMERG => syslog::Severity::LOG_EMERG,
            SyslogSeverity::ALERT => syslog::Severity::LOG_ALERT,
            SyslogSeverity::CRIT => syslog::Severity::LOG_CRIT,
            SyslogSeverity::ERR => syslog::Severity::LOG_ERR,
            SyslogSeverity::WARNING => syslog::Severity::LOG_WARNING,
            SyslogSeverity::NOTICE => syslog::Severity::LOG_NOTICE,
            SyslogSeverity::INFO => syslog::Severity::LOG_INFO,
            SyslogSeverity::DEBUG => syslog::Severity::LOG_DEBUG,
        }).unwrap_or(syslog::Severity::LOG_DEBUG) as u32
}

#[cfg(test)]
mod tests {
    use super::get_limits;
    use super::mk_healthcheck;
    use std::collections::HashMap;

    #[test]
    fn test() {
        let args = vec!["riffol", "-f", "tests/riffol.conf"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let config = super::get_config(args);
        println!("{:?}", config);
        assert!(config.is_ok());

        // test mk_healthcheck
        assert!(mk_healthcheck("unknown").is_err());
        assert!(mk_healthcheck("").is_err());
        assert!(mk_healthcheck("tcp").is_err());
        assert!(mk_healthcheck("tcp://").is_err());
        assert!(mk_healthcheck("tcp://invalid").is_err());
        assert!(mk_healthcheck("tcp://127.0.0.1:80").is_ok());
        assert!(mk_healthcheck("df://").is_err());
        assert!(mk_healthcheck("df:///dev/sda").is_err());
        assert!(mk_healthcheck("df:///dev/sda:nan").is_err());
        assert!(mk_healthcheck("df:///dev/sda:2.0").is_err());
        assert!(mk_healthcheck("df:///dev/sda:100").is_ok());
        assert!(mk_healthcheck("proc://").is_err());
        assert!(mk_healthcheck("proc://anything").is_ok());

        // test get_limits
        let limits: HashMap<String, u64> = [("max_procs".to_owned(), 64)].iter().cloned().collect();
        let config: HashMap<String, HashMap<String, u64>> =
            vec![("1".to_owned(), limits)].iter().cloned().collect();
        assert!(get_limits(&config, &vec!["2".to_owned()]).is_err());
        assert!(get_limits(&config, &vec!["1".to_owned()]).is_ok());

        let limits: HashMap<String, u64> = [("nonono".to_owned(), 64)].iter().cloned().collect();
        let config: HashMap<String, HashMap<String, u64>> =
            vec![("1".to_owned(), limits)].iter().cloned().collect();
        assert!(get_limits(&config, &vec!["1".to_owned()]).is_err());
    }
}
