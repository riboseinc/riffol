use std::io;
use std::fs::File;
use serde_json;
use std::path::Path;
use std::collections::HashMap;

#[derive(Deserialize)]
struct JSONConfig {
    init: JSONInit,
    application_groups: HashMap<String, JSONApplicationGroup>,
    applications: HashMap<String, JSONApplication>
}

#[derive(Deserialize)]
struct JSONInit {
    application_groups: Vec<String>
}

#[derive(Deserialize)]
struct JSONApplicationGroup {
    applications: Vec<String>
}

#[derive(Deserialize)]
struct JSONApplication {
    exec: String,
    #[serde(default = "default_application_start")]
    start: String,
    #[serde(default = "default_application_stop")]
    stop: String,
    #[serde(default = "default_application_restart")]
    restart: String
}

fn default_application_start() -> String { "start".to_string() }
fn default_application_stop() -> String { "stop".to_string() }
fn default_application_restart() -> String { "restart".to_string() }

pub struct Config {
    applications: Vec<Application>,
}

pub struct Application {
    exec: String,
    start: String,
    stop: String,
    restart: String
}

pub fn get_config<P: AsRef<Path>>(path: P) -> Result<Config, String> {
    let json_config = match read_config(&path) {
        Ok(c) => c,
	Err(s) => return Err(format!("Unable to read config file \"{}\": {}"
	    , path.as_ref().display(), s))
    };

    for group_name in json_config.init.application_groups {
        match json_config.application_groups.get(&group_name) {
	    Some(group) => {
	        for ap_name in &group.applications {
		    match json_config.applications.get(ap_name) {
		        Some(ap) => {},
			None => return Err(format!("No such application \"{}\"", ap_name))
		    }
		}
	    },
	    None => return Err(format!("No such application_group \"{}\"", group_name))
	}
    }

    Ok(Config{applications: vec![]})
}

fn read_config<P: AsRef<Path>>(path: P) -> io::Result<JSONConfig> {
    let json_config = serde_json::from_reader(File::open(path)?)?;
    Ok(json_config)
}