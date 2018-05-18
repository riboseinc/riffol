use std::io;
use std::fs::File;
use serde_json;
use serde_json::Value;
use std::path::Path;
use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};
use std::process::Command;
use std::iter::{Iterator};

#[derive(Deserialize)]
struct JSONConfig {
    init: JSONInit,
    application_groups: HashMap<String, JSONApplicationGroup>,
    applications: HashMap<String, JSONApplication>,
    dependencies: HashMap<String, Vec<String>>,
    #[serde(default = "default_config_health_checks")]
    health_checks: HashMap<String, JSONHealthChecks>
}

fn default_config_health_checks() -> HashMap<String, JSONHealthChecks>{ HashMap::new() }

#[derive(Deserialize)]
struct JSONInit {
    application_groups: Vec<String>
}

#[derive(Deserialize)]
struct JSONApplicationGroup {
    applications: Vec<String>,
    dependencies: Vec<String>
}

#[derive(Deserialize)]
struct JSONApplication {
    exec: String,
    #[serde(default = "default_application_start")]
    start: String,
    #[serde(default = "default_application_stop")]
    stop: String,
    #[serde(default = "default_application_restart")]
    restart: String,
    health_checks: Option<Vec<String>>
}

fn default_application_start() -> String { "start".to_string() }
fn default_application_stop() -> String { "stop".to_string() }
fn default_application_restart() -> String { "restart".to_string() }

#[derive(Deserialize)]
struct JSONHealthChecks {
    checks: Vec<HashMap<String, Value>>,
    interval: u64
}

pub struct Config {
    pub applications: Vec<Application>,
    pub dependencies: Vec<String>
}

pub struct Application {
    pub exec: String,
    pub start: String,
    pub stop: String,
    pub restart: String,
    pub health_checks: Vec<ScheduledHealthCheck>
}

pub struct ScheduledHealthCheck {
    instant: Instant,
    interval: Duration,
    health_check: Box<HealthCheck>
}

impl ScheduledHealthCheck {
    pub fn check(&mut self) -> bool {
        if Instant::now() < self.instant {
	   self.instant += self.interval;
	   return self.health_check.check();
        }
	true
    }
    pub fn get_check_instant(&self) -> Instant {
        self.instant
    }
}

pub trait HealthCheck {
    fn check(&self) -> bool;
}

pub struct DfHealthCheck {
    file: String,
    free: u64
}

impl HealthCheck for DfHealthCheck {
    fn check(&self) -> bool {
        fn avail(o: &Vec<u8>) -> Option<u64> {
	    match String::from_utf8_lossy(o).lines().skip(1).next() {
	        Some(s) => match s.trim_right_matches('M').parse::<u64>() {
		    Ok(n)  => Some(n),
		    Err(_) => None
		}
		None    => None
            }
	};

        match Command::new("/bin/df")
	              .arg("-BM")
		      .arg("--output=avail")
		      .arg(&self.file)
		      .output() {
            Ok(o) => match (o.status.success(), avail(&o.stdout)) {
	        (true, Some(n)) => self.free < n,
		_         => false
	    },
	    _     => false
	}
    }
}

pub struct ProcHealthCheck {
    process: String,
}

impl HealthCheck for ProcHealthCheck {
    fn check(&self) -> bool {
        match Command::new("/bin/pidof").arg(&self.process).status() {
	    Ok(s) if s.success() => true,
	    _                    => false
	}
    }
}

pub struct TcpHealthCheck {
    addr: SocketAddr,
    timeout: Duration
}

impl HealthCheck for TcpHealthCheck {
    fn check(&self) -> bool {
        match TcpStream::connect_timeout(&self.addr, self.timeout) {
	    Ok(_)  => true,
	    Err(_) => false
	}
    }
}

pub fn get_config<P: AsRef<Path>>(path: P) -> Result<Config, String> {
    let json_config = match read_config(&path) {
        Ok(c) => c,
	Err(s) => return Err(format!("Unable to read config file \"{}\": {}"
	    , path.as_ref().display(), s))
    };

    let mut config = Config {applications: vec![], dependencies: vec![] };
    for group_name in json_config.init.application_groups {
        match json_config.application_groups.get(&group_name) {
	    Some(group) => {
	        for ap_name in &group.applications {
		    match json_config.applications.get(ap_name) {
		        Some(ap) => {
			    let health_checks =
				match &ap.health_checks {
				    Some(cs) => {
				        match get_health_checks(&json_config.health_checks, &cs) {
					    Ok(cs) => cs,
					    Err(e) => return Err(e)
					}
				    }
				    None => vec![]
			        };
			    config.applications.push(Application {
			        exec: ap.exec.clone(),
				start: ap.start.clone(),
				stop: ap.stop.clone(),
				restart: ap.restart.clone(),
				health_checks: health_checks
			    })
			},
			None => return Err(format!("No such application \"{}\"", ap_name))
		    }
		}
		for dep_name in &group.dependencies {
		    match json_config.dependencies.get(dep_name) {
		        Some(dep) => dep.iter().for_each(|d| config.dependencies.push(d.clone())),
			None => return Err(format!("No such dependencies \"{}\"", dep_name))
		    }
		}
	    },
	    None => return Err(format!("No such application_group \"{}\"", group_name))
	}
    }

    Ok(config)
}

fn read_config<P: AsRef<Path>>(path: P) -> io::Result<JSONConfig> {
    let json_config = serde_json::from_reader(File::open(path)?)?;
    Ok(json_config)
}

fn get_health_checks(
    configs: &HashMap<String, JSONHealthChecks>,
    checks: &Vec<String>
    ) -> Result<Vec<ScheduledHealthCheck>, String>
{
    let mut result = vec![];

    for check in checks.iter() {
        match configs.get(check) {
	    Some(config) => {
	        for p in config.checks.iter() {
		    match mk_health_check(p) {
		        Ok(x) => result.push(ScheduledHealthCheck {
			    instant: Instant::now() + Duration::from_secs(config.interval),
			    interval: Duration::from_secs(config.interval),
			    health_check: x
			}),
			Err(e) => return Err(e)
		    }
		}
	    },
	    None => return Err(format!("No such health_check \"{}\"", check))
	}
    }

    Ok(result)
}

fn mk_health_check(params: &HashMap<String, Value>)
    -> Result<Box<HealthCheck>, String> {
    match params.get("type") {
        Some(Value::String(t)) => {
	    match t.as_ref() {
	        "proc" => mk_proc_health_check(params),
	        "df" => mk_df_health_check(params),
		"tcp" => mk_tcp_health_check(params),
                _ => Err(format!("Unknown health_check type \"{}\"", t))
	    }
	},
	// TODO use serde deserialize_with to catch bad health_checks
	Some(t) => Err(format!("Unknown health_check type \"{:?}\"", t)),
	_ => Err(String::from("Health_Check configuration with no \"type\" field"))
    }
}

fn mk_proc_health_check(params: &HashMap<String, Value>) -> Result<Box<HealthCheck>, String> {
    match params.get("proc") {
        Some(Value::String(p)) => Ok(Box::new(ProcHealthCheck{ process: p.clone() })),
	_ => Err(String::from("Bad proc health_check. Use \"proc\" : \"<process_name>\""))
    }
}

fn mk_df_health_check(params: &HashMap<String, Value>) -> Result<Box<HealthCheck>, String> {
    match (params.get("file"), params.get("free")) {
        (Some(Value::String(file)), Some(Value::Number(free))) if free.is_i64() =>
	    Ok(Box::new(DfHealthCheck{ file: file.clone(), free: free.as_i64().unwrap() as u64 })),
	_ => Err(String::from("Bad df health_check. Use \"file\" :\"<filename>\", \"free\" : <mb>"))
    }
}

fn mk_tcp_health_check(params: &HashMap<String, Value>) -> Result<Box<HealthCheck>, String> {
    match (params.get("addr"), params.get("timeout")) {
        (Some(Value::String(addr)), Some(Value::Number(timeout))) if timeout.is_i64() => {
	    match addr.parse() {
	        Ok(addr) => Ok(Box::new(TcpHealthCheck {
	            addr: addr,
		    timeout: Duration::from_secs(timeout.as_i64().unwrap() as u64)
	        })),
                _ => Err(String::from("Bad tcp health_check. Malformed address"))
	    }
	},
	_ => Err(String::from("Bad tcp health_check. Use \"addr\" :\"<address>\", \"timeout\" : <seconds>"))
    }
}