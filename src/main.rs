extern crate riffol;

use riffol::config::{get_config, Application};
use std::process::Command;
use std::{env, thread};
use std::time::{Instant};

fn main() {
    let arg0 = env::args().next().unwrap();

    let config_path = match env::args().skip(1).next() {
        Some(p) => p,
	None => String::from("./riffol.conf")
    };

    let mut config = match get_config(config_path) {
        Ok(mut c) => c,
	Err(s) => {
	    println!("{}: {}", arg0, s);
	    return ();
	}
    };

    config.dependencies.iter().for_each(|d| {
        let result = Command::new("apt-get")
			     .arg("-y")
			     .arg("--no-install-recommends")
			     .arg("install")
			     .arg(d)
                             .status();
        if result.is_err() || !result.unwrap().success() {
            println!("{}: Failed to install dependency \"{}\"", arg0, d);
            return ()
	}
    });

    {
        let (running, failed)
        : (Vec<Option<&Application>>, Vec<Option<&Application>>)
        = config.applications.iter().map(|ap| {
            let result = Command::new(&ap.exec)
	                         .arg(&ap.start)
	                          .spawn();
            match result {
                Ok(_)  => {
	            println!("{}: Successfully spawned {}", arg0, ap.exec);
		    Some(ap)
	        },
	        Err(_) => {
	            println!("{}: Failed to spawn {}", arg0, ap.exec);
	            None
	        }
	    }
        }).partition(|o| o.is_some());

        if failed.len() != 0 {
            running.iter().rev().map(|o| o.unwrap()).for_each(|ap| {
                println!("Stopping {}", ap.exec);
	        Command::new(&ap.exec)
	                .arg(&ap.stop)
	                .spawn()
		        .ok();
            });
	    return ();
        }
    }

    loop {
        for ap in &mut config.applications {
            for health_check in &mut ap.health_checks {
	        if !health_check.check() {
		    // TODO: health_checkfail
		}
	    }
	}

	let next_check_instant = config.applications.iter()
	.fold(vec![], |mut a, ap| {
	    for c in &ap.health_checks {
	        a.push(c.get_check_instant());
	    }
	    a
	}).iter().min().unwrap().clone();

	let now = Instant::now();

	if next_check_instant > now {
            thread::sleep(now - next_check_instant);
	}
    }
}