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
// ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
// OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
// LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
// DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

extern crate crossbeam_channel;
extern crate libc;
extern crate mio;
extern crate nereon;
extern crate nix;
extern crate rand;
extern crate signal_hook;
extern crate slab;
extern crate syslog;

#[macro_use]
extern crate nereon_derive;

#[macro_use]
extern crate log;

mod application;
mod config;
mod distro;
mod health;
mod init;
mod limit;
mod signal;
mod stream;
mod timers;

pub fn riffol<T: std::iter::IntoIterator<Item = String>>(args: T) -> Result<(), String> {
    let config::Riffol {
        applications: apps,
        dependencies: deps,
        healthchecks: checks,
    } = config::get_config(args)?;

    distro::install_packages(&deps)?;

    let sig_recv = signal::recv_signals();
    let check_recv = health::recv_checks(&checks);
    init::Init::run(apps, &sig_recv, &check_recv);
    Ok(())
}
