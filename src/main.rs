extern crate riffol;

use riffol::config::{get_config, Application};
use riffol::health::{HealthCheck, IntervalHealthCheck};
use std::process::Command;
use std::{env, thread};
use std::time::{Instant, Duration};

struct Check<'a> {
    the_check: &'a IntervalHealthCheck,
    application: &'a Application,
    instant: Instant,
}

fn main() {
    let arg0 = env::args().next().unwrap();

    let config_path = match env::args().skip(1).next() {
        Some(p) => p,
        None => String::from("./riffol.conf"),
    };

    let config = match get_config(config_path) {
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
            return ();
        }
    });

    let (running, failed): (Vec<Option<&Application>>, Vec<Option<&Application>>) =
        config
            .applications
            .iter()
            .map(|ap| {
                let result = Command::new(&ap.exec).arg(&ap.start).spawn();
                match result {
                    Ok(_) => {
                        println!("{}: Successfully spawned {}", arg0, ap.exec);
                        Some(ap)
                    }
                    Err(_) => {
                        println!("{}: Failed to spawn {}", arg0, ap.exec);
                        None
                    }
                }
            })
            .partition(|o| o.is_some());

    if failed.len() != 0 {
        running.iter().rev().map(|o| o.unwrap()).for_each(|ap| {
            println!("Stopping {}", ap.exec);
            Command::new(&ap.exec).arg(&ap.stop).spawn().ok();
        });
        return ();
    }

    // build vector of Checks
    let mut checks = config.applications.iter().fold(vec![], |mut a, v| {
        for c in v.health_checks.iter() {
            a.push(Check {
                application: v,
                the_check: c,
                instant: Instant::now() + *c.get_interval(),
            });
        }
        a
    });

    loop {
        let now = Instant::now();
        match checks.iter_mut().min_by(|x, y| x.instant.cmp(&y.instant)) {
            Some(check) => {
                if now <= check.instant {
                    thread::sleep(check.instant - now);
                }
                check.instant += *(check.the_check).get_interval();
                if !check.the_check.do_check() {}
            }
            _ => thread::sleep(Duration::from_secs(1)),
        }
    }
}
