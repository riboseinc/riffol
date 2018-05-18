use std::io;
use std::fs::File;
use serde_json;
use serde_json::Value;
use std::path::Path;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[derive(Deserialize)]
struct JSONConfig {
    init: JSONInit,
    application_groups: HashMap<String, JSONApplicationGroup>,
    applications: HashMap<String, JSONApplication>,
    dependencies: HashMap<String, Vec<String>>,
    #[serde(default = "default_config_healthchecks")]
    healthchecks: HashMap<String, JSONHealthChecks>
}

fn default_config_healthchecks() -> HashMap<String, JSONHealthChecks>{ HashMap::new() }

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
struct JSONHealthChecks {
    checks: Vec<HashMap<String, Value>>,
    interval: i32
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
    pub health_checks: Vec<Box<HealthCheck>>
}

pub trait HealthCheck {
    fn check(&self) -> bool;
}

pub struct DFHealthCheck {
    device: String,
    free: i64
}

impl HealthCheck for DFHealthCheck {
    fn check(&self) -> bool {
        true
    }
}

pub struct ProcHealthCheck {
    process: String,
}

impl HealthCheck for ProcHealthCheck {
    fn check(&self) -> bool {
        true
    }
}

pub struct TCPHealthCheck {
    addr: SocketAddr,
    timeout: i32
}

impl HealthCheck for TCPHealthCheck {
    fn check(&self) -> bool {
        true
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
				match &ap.healthchecks {
				    Some(cs) => {
				        match get_health_checks(&json_config.healthchecks, &cs) {
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
    ) -> Result<Vec<Box<HealthCheck>>, String>
{
    let mut result = vec![];

    for check in checks.iter() {
        match configs.get(check) {
	    Some(config) => {
	        for p in config.checks.iter() {
		    match mk_health_check(p) {
		        Ok(x) => result.push(x),
			Err(e) => return Err(e)
		    }
		}
	    },
	    None => return Err(format!("No such healthcheck \"{}\"", check))
	}
    }

    Ok(result)
}

fn mk_health_check(params: &HashMap<String, Value>)
    -> Result<Box<HealthCheck>, String> {
    match params.get("type") {
        Some(Value::String(t)) => {
	    match t.as_ref() {
	        "proc" => Ok(Box::new(ProcHealthCheck {process: String::from("str")})),
	        "df" => Ok(Box::new(DFHealthCheck {device: String::from("f"), free: 256})),
		"tcp" => Ok(Box::new(TCPHealthCheck {addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080), timeout: 256})),
                _ => Err(format!("Unknown healthcheck type \"{}\"", t))
	    }
	},
	// TODO use serde deserialize_with to catch bad healthchecks
	Some(t) => Err(format!("Unknown healthcheck type \"{:?}\"", t)),
	_ => Err(String::from("Healthcheck configuration with no \"type\" field"))
    }
}