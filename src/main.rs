extern crate serde;
extern crate serde_json;

#[macro_use]
extern crate serde_derive;

use std::io;
use serde_json::{Value, Error};

#[derive(Deserialize)]
struct Config {
    init: Init,
    application_groups: Vec<ApplicationGroup>,
    applications: Vec<Application>
}

#[derive(Deserialize)]
struct Init {
    name: String,
    application_groups: Vec<String>
}

#[derive(Deserialize)]
struct ApplicationGroup {
    name: String,
    applications: Vec<String>
}

#[derive(Deserialize)]
struct Application {
    name: String,
    exec: String
}

fn main() -> io::Result<()> {
    let config: Config = serde_json::from_reader(std::fs::File::open("example.conf")?)?;
    Ok(())
}