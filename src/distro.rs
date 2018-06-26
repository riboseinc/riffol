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

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn which(cmd: &str) -> bool {
    env::var("PATH")
        .unwrap_or("".to_owned())
        .split(":")
        .map(|d| match fs::metadata(Path::new(d).join(Path::new(cmd))) {
            Ok(m) => m.is_file() && (m.permissions().mode() & 0o111) != 0,
            Err(_) => false,
        })
        .any(|x| x)
}

fn get_installer() -> Box<Fn(&str) -> bool> {
    match ["apt", "yum"]
        .iter()
        .find(|pm| which(pm))
        .unwrap_or(&"")
        .as_ref()
    {
        "apt" => Box::new(|pkg| {
            Command::new("apt-get")
                .arg("-y")
                .arg("--no-install-recommends")
                .arg("install")
                .arg(&pkg)
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }),
        "yum" => Box::new(|pkg| {
            Command::new("yum")
                .arg("-y")
                .arg("install")
                .arg(&pkg)
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }),
        _ => Box::new(|_| false),
    }
}

pub fn install_packages(depends: &Vec<String>) -> Result<(), String> {
    let install = get_installer();
    for d in depends {
        if !install(d) {
            return Err(format!("Couldn't install dependency: {}", d));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_which() {
        assert_eq!(super::which("ls"), true);
        assert_eq!(super::which("lllsss"), false);
    }
}
