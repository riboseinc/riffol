extern crate riffol;

use riffol::config::{get_config, Application};
use std::process::Command;

fn main() {
    let config = match get_config("riffol.conf") {
        Ok(c) => c,
	Err(s) => {
	    println!("{}", s);
	    return ();
	}
    };

    let (running, failed): (Vec<Option<&Application>>, Vec<Option<&Application>>) = config.applications.iter().map(|ap| {
        let result = Command::new(format!("{} {}", ap.exec, ap.start))
	                     .spawn();
        match result {
	    Ok(_)  => {
	        println!("Successfully spawned {}", ap.exec);
		Some(ap)
	    },
	    Err(_) => {
	        println!("Failed to spawn {}", ap.exec);
	        None
	    }
	}
    }).partition(|o| o.is_some());

    if failed.len() != 0 {
        running.iter().rev().map(|o| o.unwrap()).for_each(|ap| {
            println!("Stopping {}", ap.exec);
	    Command::new(format!("{} {}", ap.exec, ap.stop))
	            .spawn()
		    .ok();
        });
	return ();
    }
}