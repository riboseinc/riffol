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
extern crate libc;
extern crate serde_json;

#[macro_use]
extern crate serde_derive;

mod application;
mod config;
mod distro;
mod health;
mod init;
mod limit;
mod stream;

use chan_signal::Signal;
use std::env;
use std::process::exit;

pub fn riffol<T: std::iter::IntoIterator<Item = String>>(args: T) {
    let config::Riffol {
        applications: apps,
        dependencies: deps,
    } = config::get_config(args).unwrap_or_else(fail);

    distro::install_packages(&deps).unwrap_or_else(fail);

    let mut signals = vec![];

    #[cfg(target_os = "linux")]
    {
        static PR_SET_CHILD_SUBREAPER: libc::c_int = 36;

        if unsafe { libc::getpid() } != 1 {
            if unsafe { libc::prctl(PR_SET_CHILD_SUBREAPER, 1) } == 0 {
                signals.push(Signal::CHLD);
            } else {
                eprintln!(
                    "{}: Not PID 1 and couldn't set PR_CHILD_SUBREAPER",
                    progname(),
                );
            }
        }
    }

    signals.push(Signal::INT);
    signals.push(Signal::TERM);
    let signal = chan_signal::notify(signals.as_ref());

    let mut init = init::Init::new(apps);

    init.start().unwrap_or_else(fail);

    loop {
        let s = signal.recv().unwrap();
        eprintln!("{}: Received signal {:?}", progname(), s);

        match s {
            Signal::CHLD => unsafe {
                libc::wait(std::ptr::null_mut());
            },
            _ => break,
        }
    }

    init.stop();
}

fn progname() -> String {
    match env::current_exe() {
        Ok(name) => name.as_path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        Err(_) => "(unknown)".to_owned(),
    }
}

fn fail<T>(e: String) -> T {
    eprintln!("{}: {}", progname(), e);
    exit(1);
}
