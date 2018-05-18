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
    #[serde(default = "default_config_healthchecks")]
    healthchecks: HashMap<String, JSONHealthchecks>
}

fn default_config_healthchecks() -> HashMap<String, JSONHealthchecks>{ HashMap::new() }

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
    healthchecks: Option<Vec<String>>
}

fn default_application_start() -> String { "start".to_string() }
fn default_application_stop() -> String { "stop".to_string() }
fn default_application_restart() -> String { "restart".to_string() }

#[derive(Deserialize)]
struct JSONHealthchecks {
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
    pub healthchecks: Vec<ScheduledHealthcheck>
}

pub struct ScheduledHealthcheck {
    instant: Instant,
    interval: Duration,
    healthcheck: Box<Healthcheck>
}

impl ScheduledHealthcheck {
    pub fn check(&mut self) -> bool {
        if Instant::now() < self.instant {
	   self.instant += self.interval;
	   return self.healthcheck.check();
        }
	true
    }
}

pub trait Healthcheck {
    fn check(&self) -> bool;
}

pub struct DfHealthcheck {
    file: String,
    free: u64
}

impl Healthcheck for DfHealthcheck {
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

pub struct ProcHealthcheck {
    process: String,
}

impl Healthcheck for ProcHealthcheck {
    fn check(&self) -> bool {
        match Command::new("/bin/pidof").arg(&self.process).status() {
	    Ok(s) if s.success() => true,
	    _                    => false
	}
    }
}

pub struct TcpHealthcheck {
    addr: SocketAddr,
    timeout: Duration
}

impl Healthcheck for TcpHealthcheck {
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
			    let healthchecks =
				match &ap.healthchecks {
				    Some(cs) => {
				        match get_healthchecks(&json_config.healthchecks, &cs) {
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
				healthchecks: healthchecks
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

fn get_healthchecks(
    configs: &HashMap<String, JSONHealthchecks>,
    checks: &Vec<String>
    ) -> Result<Vec<ScheduledHealthcheck>, String>
{
    let mut result = vec![];

    for check in checks.iter() {
        match configs.get(check) {
	    Some(config) => {
	        for p in config.checks.iter() {
		    match mk_healthcheck(p) {
		        Ok(x) => result.push(ScheduledHealthcheck {
			    instant: Instant::now() + Duration::from_secs(config.interval),
			    interval: Duration::from_secs(config.interval),
			    healthcheck: x
			}),
			Err(e) => return Err(e)
		    }
		}
	    },
	    None => return Err(format!("No such healthcheck \"{}\"", check))
	}
    }

    Ok(result)
}

fn mk_healthcheck(params: &HashMap<String, Value>)
    -> Result<Box<Healthcheck>, String> {
    match params.get("type") {
        Some(Value::String(t)) => {
	    match t.as_ref() {
	        "proc" => mk_proc_healthcheck(params),
	        "df" => mk_df_healthcheck(params),
		"tcp" => mk_tcp_healthcheck(params),
                _ => Err(format!("Unknown healthcheck type \"{}\"", t))
	    }
	},
	// TODO use serde deserialize_with to catch bad healthchecks
	Some(t) => Err(format!("Unknown healthcheck type \"{:?}\"", t)),
	_ => Err(String::from("Healthcheck configuration with no \"type\" field"))
    }
}

fn mk_proc_healthcheck(params: &HashMap<String, Value>) -> Result<Box<Healthcheck>, String> {
    match params.get("proc") {
        Some(Value::String(p)) => Ok(Box::new(ProcHealthcheck{ process: p.clone() })),
	_ => Err(String::from("Bad proc healthcheck. Use \"proc\" : \"<process_name>\""))
    }
}

fn mk_df_healthcheck(params: &HashMap<String, Value>) -> Result<Box<Healthcheck>, String> {
    match (params.get("file"), params.get("free")) {
        (Some(Value::String(file)), Some(Value::Number(free))) if free.is_i64() =>
	    Ok(Box::new(DfHealthcheck{ file: file.clone(), free: free.as_i64().unwrap() as u64 })),
	_ => Err(String::from("Bad df healthcheck. Use \"file\" :\"<filename>\", \"free\" : <mb>"))
    }
}

fn mk_tcp_healthcheck(params: &HashMap<String, Value>) -> Result<Box<Healthcheck>, String> {
    match (params.get("addr"), params.get("timeout")) {
        (Some(Value::String(addr)), Some(Value::Number(timeout))) if timeout.is_i64() => {
	    match addr.parse() {
	        Ok(addr) => Ok(Box::new(TcpHealthcheck {
	            addr: addr,
		    timeout: Duration::from_secs(timeout.as_i64().unwrap() as u64)
	        })),
                _ => Err(String::from("Bad tcp healthcheck. Malformed address"))
	    }
	},
	_ => Err(String::from("Bad tcp healthcheck. Use \"addr\" :\"<address>\", \"timeout\" : <seconds>"))
    }
}