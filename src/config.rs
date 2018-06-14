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

extern crate nereon;

use application::{self, AppAction, AppState};
use health::{DfCheck, HealthCheck, IntervalHealthCheck, ProcCheck, TcpCheck};
use limit::{Limit, RLimit};
use serde_json;
use std::collections::HashMap;
use std::iter::Iterator;
use std::path::Path;
use std::time::Duration;

#[derive(Deserialize)]
struct Config {
    init: HashMap<String, Init>,
    application_group: HashMap<String, AppGroup>,
    application: HashMap<String, Application>,
    dependency: HashMap<String, Dependencies>,
    #[serde(default = "default_config_healthchecks")]
    healthchecks: HashMap<String, HealthChecks>,
    #[serde(default = "default_config_limits")]
    limits: HashMap<String, Limits>,
}

fn default_config_healthchecks() -> HashMap<String, HealthChecks> {
    HashMap::new()
}

fn default_config_limits() -> HashMap<String, Limits> {
    HashMap::new()
}

#[derive(Deserialize)]
struct Init {
    application_groups: Vec<String>,
}

#[derive(Deserialize)]
struct AppGroup {
    applications: Vec<String>,
    dependencies: Vec<String>,
}

#[derive(Deserialize)]
struct Application {
    exec: String,
    #[serde(default = "default_application_start")]
    start: String,
    #[serde(default = "default_application_stop")]
    stop: String,
    #[serde(default = "default_application_restart")]
    restart: String,
    healthchecks: Option<Vec<String>>,
    #[serde(default = "default_application_healthcheckfail")]
    healthcheckfail: AppAction,
    limits: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Dependencies {
    packages: Vec<String>,
}

type Limits = HashMap<String, u64>;

fn default_application_start() -> String {
    "start".to_string()
}

fn default_application_stop() -> String {
    "stop".to_string()
}

fn default_application_restart() -> String {
    "restart".to_string()
}

fn default_application_healthcheckfail() -> AppAction {
    AppAction::Restart
}

#[derive(Deserialize)]
struct HealthChecks {
    checks: Vec<String>,
    timeout: u64,
    interval: u64,
}

#[derive(Debug)]
pub struct Riffol {
    pub applications: Vec<application::Application>,
    pub dependencies: Vec<String>,
}

pub fn get_config<T: IntoIterator<Item = String>>(args: T) -> Result<Riffol, String> {
    let options = vec![
        nereon::Opt::new(
            "",
            Some("f"),
            Some("file"),
            Some("RIFFOL_CONFIG"),
            0,
            None,
            Some("@{}"),
            Some("Configuration file"),
        ),
    ];

    let config = match nereon::nereon_json(options, args) {
        Ok(c) => c,
        Err(s) => return Err(format!("Couldn't get config: {}", s)),
    };

    let config = match serde_json::from_str::<Config>(&config) {
        Ok(c) => c,
        Err(e) => return Err(format!("Invalid config: {}", e)),
    };

    let mut riffol = Riffol {
        applications: vec![],
        dependencies: vec![],
    };

    for (_, init) in config.init {
        for group_name in init.application_groups {
            match config.application_group.get(&group_name) {
                Some(group) => {
                    for ap_name in &group.applications {
                        match config.application.get(ap_name) {
                            Some(ap) => {
                                let healthchecks = match &ap.healthchecks {
                                    Some(cs) => match get_healthchecks(&config.healthchecks, &cs) {
                                        Ok(cs) => cs,
                                        Err(e) => return Err(e),
                                    },
                                    None => vec![],
                                };
                                let limits = match &ap.limits {
                                    Some(ls) => match get_limits(&config.limits, &ls) {
                                        Ok(ls) => ls,
                                        Err(e) => return Err(e),
                                    },
                                    None => vec![],
                                };
                                riffol.applications.push(application::Application {
                                    exec: ap.exec.clone(),
                                    start: ap.start.clone(),
                                    stop: ap.stop.clone(),
                                    restart: ap.restart.clone(),
                                    healthchecks: healthchecks,
                                    healthcheckfail: ap.healthcheckfail.clone(),
                                    limits: limits,
                                    checks: vec![],
                                    state: AppState::Stopped,
                                })
                            }
                            None => return Err(format!("No such application \"{}\"", ap_name)),
                        }
                    }
                    for dep_name in &group.dependencies {
                        match config.dependency.get(dep_name) {
                            Some(dep) => dep.packages
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

    Ok(riffol)
}

fn get_healthchecks(
    configs: &HashMap<String, HealthChecks>,
    checks: &Vec<String>,
) -> Result<Vec<IntervalHealthCheck>, String> {
    let mut result = vec![];

    for check in checks.iter() {
        match configs.get(check) {
            Some(config) => {
                for p in config.checks.iter() {
                    match mk_healthcheck(p) {
                        Ok(x) => result.push(IntervalHealthCheck::new(
                            Duration::from_secs(config.interval),
                            Duration::from_secs(config.timeout),
                            x,
                        )),
                        Err(e) => return Err(e),
                    }
                }
            }
            None => return Err(format!("No such healthcheck \"{}\"", check)),
        }
    }

    Ok(result)
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
            ref s if s.len() > 0 => Ok(HealthCheck::ProcCheck(ProcCheck::new(&args))),
            _ => bad("<process>"),
        },
        "df" => {
            let bad_df = || bad("<file>:<free>");
            match split2(":", &args) {
                (ref file, ref free) if file.len() > 0 => match free.parse() {
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

fn get_limits(
    configs: &HashMap<String, Limits>,
    limits: &Vec<String>,
) -> Result<Vec<RLimit>, String> {
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

#[cfg(test)]
mod tests {
    use super::get_limits;
    use super::mk_healthcheck;
    use std::collections::HashMap;

    #[test]
    fn test() {
        let args = vec!["-f", "riffol.conf"]
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

        /*
        let limits: HashMap<String, u64> = [
            ("max_procs".to_owned(), 64),
            ("max_procs".to_owned(), 32),
        ].iter().cloned().collect();
        let config: HashMap<String, HashMap<String, u64>> = vec![
            ("1".to_owned(), limits)
        ].iter().cloned().collect();
        assert_eq!(
            get_limits(&config, &vec!["1".to_owned()]),
            Ok(vec![
                RLimit::Procs(Limit::Num(32)),
                RLimit::Memory(Limit::Infinity),
                RLimit::Files(Limit::Infinity),
                ]
                ).is_err());
*/
    }
}
