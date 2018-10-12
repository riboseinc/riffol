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

use std::time::{Duration, Instant};

struct Timer<T> {
    instant: Instant,
    payload: T,
}

pub struct Timers<T> {
    timers: Vec<(Timer<T>)>,
}

impl<T> Timers<T> {
    pub fn new() -> Self {
        Timers { timers: Vec::new() }
    }

    pub fn add_timer(&mut self, duration: Duration, payload: T) {
        self.timers.push(Timer {
            instant: Instant::now() + duration,
            payload,
        });
    }

    pub fn get_timeout(&self) -> Option<Duration> {
        self.earliest()
            .map(|Timer { instant, .. }| Instant::now() - *instant)
    }

    pub fn remove_earliest(&mut self) -> T {
        let position = self
            .earliest()
            .and_then(|e| self.timers.iter().position(|t| t.instant == e.instant))
            .expect("remove_earliest called when empty");
        self.timers.swap_remove(position).payload
    }

    fn earliest(&self) -> Option<&Timer<T>> {
        self.timers.iter().min_by(|a, b| a.instant.cmp(&b.instant))
    }
}
