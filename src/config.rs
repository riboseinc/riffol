use std::io;
use std::fs::File;
use serde_json;
use std::path::Path;

#[derive(Deserialize)]
struct JSONConfig {
    init: JSONInit,
    application_groups: Vec<JSONApplicationGroup>,
    applications: Vec<JSONApplication>
}

#[derive(Deserialize)]
struct JSONInit {
    name: String,
    application_groups: Vec<String>
}

#[derive(Deserialize)]
struct JSONApplicationGroup {
    name: String,
    applications: Vec<String>
}

#[derive(Deserialize)]
struct JSONApplication {
    name: String,
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

pub fn get_config<P: AsRef<Path>>(path: P) -> io::Result<Config> {
    let json_config: JSONConfig = serde_json::from_reader(File::open("riffol.conf")?)?;
    let app_group_names = &json_config.init.application_groups;
    let json_app_groups = app_group_names.iter()
    .map(|name| {
        json_config.application_groups.iter()
	.find(|g| &g.name == name)
    });
        
    Ok(Config{applications: vec![]})
}
