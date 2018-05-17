use std::io;
use std::fs::File;
use serde_json;
use std::path::Path;
use std::collections::HashMap;

#[derive(Deserialize)]
struct JSONConfig {
    init: JSONInit,
    application_groups: HashMap<String, JSONApplicationGroup>,
    applications: HashMap<String, JSONApplication>,
    dependencies: HashMap<String, Vec<String>>
}

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
    restart: String
}

fn default_application_start() -> String { "start".to_string() }
fn default_application_stop() -> String { "stop".to_string() }
fn default_application_restart() -> String { "restart".to_string() }

pub struct Config {
    pub applications: Vec<Application>,
    pub dependencies: Vec<String>
}

pub struct Application {
    pub exec: String,
    pub start: String,
    pub stop: String,
    pub restart: String
}

pub fn get_config<P: AsRef<Path>>(path: P) -> Result<Config, String> {
    let json_config = match read_config(&path) {
        Ok(c) => c,
	Err(s) => return Err(format!("Unable to read config file \"{}\": {}"
	    , path.as_ref().display(), s))
    };

    let mut config = Config { applications: vec![], dependencies: vec![] };
    for group_name in json_config.init.application_groups {
        match json_config.application_groups.get(&group_name) {
	    Some(group) => {
	        for ap_name in &group.applications {
		    match json_config.applications.get(ap_name) {
		        Some(ap) => {
			    config.applications.push(Application {
			        exec: ap.exec.clone(),
				start: ap.start.clone(),
				stop: ap.stop.clone(),
				restart: ap.restart.clone()
			    })},
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