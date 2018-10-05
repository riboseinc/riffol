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

use crossbeam_channel as cc;
use signal_hook;
use std::thread;

pub fn recv_signals() -> cc::Receiver<i32> {
    // set us up to adopt zombies from subprocesses
    #[cfg(target_os = "linux")]
    use libc;
    {
        static PR_SET_CHILD_SUBREAPER: libc::c_int = 36;

        if unsafe { libc::getpid() != 1 && libc::prctl(PR_SET_CHILD_SUBREAPER, 1) != 0 } {
            warn!("Couldn't set PR_CHILD_SUBREAPER",);
        }
    }

    let signals = [
        signal_hook::SIGINT,
        signal_hook::SIGTERM,
        signal_hook::SIGCHLD,
    ];
    let signals = signal_hook::iterator::Signals::new(&signals).unwrap();
    let (sig_send, sig_recv) = cc::unbounded();
    thread::spawn(move || {
        for signal in signals.forever() {
            sig_send.send(signal);
        }
    });
    sig_recv
}
