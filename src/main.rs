extern crate riffol;

use riffol::config::{get_config, Config};
use std::io;

fn main() {
    let config = match get_config("riffol.conf") {
        Ok(c) => c,
	Err(s) => {
	    println!("{}", s);
	    return ();
	}
    };
}