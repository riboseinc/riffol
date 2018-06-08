// Copyright (c) 2018, [Ribose Inc](https://www.ribose.com).
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
// 1. Redistributions of source code must retain the above copyright
//    notice, this list of conditions and the following disclaimer.
// 2. Redistributions in binary form must reproduce the above copyright
//    notice, this list of conditions and the following disclaimer in the
//    documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NO/T
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
// OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
// LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
// DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

extern crate chan_signal;
extern crate riffol;

use chan_signal::Signal;
use riffol::config::{get_config, Config};
use riffol::init::Init;
use std::env;
use std::process::Command;

fn main() {
    let arg0 = env::args().next().unwrap();

    let signal = chan_signal::notify(&[Signal::INT, Signal::TERM]);

    let Config {
        applications: apps,
        dependencies: deps,
    } = match get_config(env::args()) {
        Ok(c) => c,
        Err(s) => {
            eprintln!("{}: {}", arg0, s);
            return ();
        }
    };

    deps.iter().for_each(|d| {
        let result = Command::new("apt-get")
            .arg("-y")
            .arg("--no-install-recommends")
            .arg("install")
            .arg(d)
            .status();
        if result.is_err() || !result.unwrap().success() {
            eprintln!("{}: Failed to install dependency \"{}\"", arg0, d);
            return ();
        }
    });

    let mut init = Init::new(apps);
    match init.start() {
        Ok(_) => match signal.recv() {
            Some(s) => {
                eprintln!("{}: Received signal {:?}", arg0, s);
                init.stop();
            }
            None => (),
        },
        Err(s) => eprintln!("{}: {}", arg0, s),
    }
}
