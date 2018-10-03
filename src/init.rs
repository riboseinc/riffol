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

use application::{AppState, Application};
use crossbeam_channel as cc;
use stream;

pub struct Init {
    applications: Vec<Application>,
    stream_handler: stream::Handler,
}

impl Init {
    pub fn run(
        applications: Vec<Application>,
        sig_recv: cc::Receiver<i32>,
        fail_recv: cc::Receiver<(String, String)>,
    ) {
        let mut init = Self {
            applications,
            stream_handler: stream::Handler::new(),
        };
        // main event loop
        loop {
            if init
                .applications
                .iter()
                .all(|a| a.state == AppState::Stopped)
            {
                break;
            }
            let mut signal = None;
            let mut fail: Option<(String, String)> = None;
            cc::Select::new()
                .recv(&sig_recv, |s| signal = s)
                .recv(&fail_recv, |f| fail = f)
                .wait();
            if let Some(signal) = signal {
                init.handle_signal(signal);
            }
            if let Some((group, message)) = fail {
                init.handle_fail(&group, &message)
            }
        }
    }

    fn handle_signal(&mut self, _sig: i32) {
        unimplemented!()
    }

    fn handle_fail(&mut self, _group: &str, _message: &str) {
        unimplemented!()
    }
}
