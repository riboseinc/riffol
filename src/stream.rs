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
use mio::unix::{EventedFd, UnixReady};
use mio::{Events, Poll, PollOpt, Ready, Token};
use nix::fcntl::{fcntl, FcntlArg::F_SETFL, OFlag};
use slab::Slab;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};
use std::thread;
use std::time::Duration;
use syslog::{self, Formatter3164, Logger, LoggerBackend, Severity::*};

/// Address used for various flavours of Syslog.
#[derive(Debug, Clone)]
pub enum Address {
    Tcp(SocketAddr),
    Udp {
        server: SocketAddr,
        local: SocketAddr,
    },
    Unix(Option<String>),
}

/// Stream descriptions. Currently supported are `Syslog`
/// (TCP/UDP/Unix) and files
#[derive(Debug, Clone)]
pub enum Stream {
    File {
        filename: String,
    },
    Syslog {
        address: Address,
        facility: syslog::Facility,
        severity: u32, // syslog::Severity doesn't implement Debug,
    },
    #[cfg(test)]
    Stdout,
}

/// `Connection` is used to associate a source `fd` with a `Stream` description
struct Connection {
    source: BufReader<File>,
    sink: Stream,
}

impl Connection {
    /// Associate `fd` with `Stream`: first ensures the `fd` is set to
    /// non-blocking, then converts it into a `BufReader<File>`
    fn new(fd: RawFd, stream: Stream) -> Connection {
        // set fd to non-blocking and convert to File
        fcntl(fd, F_SETFL(OFlag::O_NONBLOCK)).unwrap(); // TODO: check result
        Connection {
            source: BufReader::new(unsafe { File::from_raw_fd(fd) }),
            sink: stream,
        }
    }
}

/// One `Handler` is used to asynchronously stream many `fd`s to
/// destinations described by `Stream` structs. It uses a thread with
/// a `mio:Poll` loop to read. Data is syncronously written to
/// relevent destinations.
///
/// Note: Sychronous writes might cause blocking (especially TCP
/// syslog writes) and may change to async in the future.
///
/// Note: Write endpoints are constructed and torn down for each write
/// (ie. TCP connect/send/close and file open/write/close). This is
/// highly inefficient but saves having to maintain TCP connections
/// and has avoids writing to unlinked (eg. `logrotate`d) files.
pub struct Handler {
    channel: cc::Sender<Message>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Handler {
    /// Creates a `Handler` with an mpsc communication channel and
    /// background thread to shuffle data between standard streams and
    /// destintions specified by `Stream`
    pub fn new() -> Handler {
        let (tx, rx) = cc::unbounded();
        Handler {
            channel: tx,
            thread: Some(thread::spawn(move || {
                handler(&rx);
            })),
        }
    }

    /// Sends a message to background thread to monitor source `fd` and
    /// write to `Stream`
    pub fn add_stream(&self, fd: RawFd, stream: &Stream) {
        // send message to handler thread
        println!("Adding fd {}", fd);
        self.channel.send(Message::Add(fd, stream.clone()))
    }
}

/// Before `Handler` is `Drop`ped, send the close message
/// to the mio thread and wait for it to finish.
impl Drop for Handler {
    fn drop(&mut self) {
        self.channel.send(Message::Close);
        self.thread.take().unwrap().join().unwrap();
    }
}

/// Message type for communication between `Handler` instance and
/// its background logging thread.
enum Message {
    Add(RawFd, Stream),
    Close,
}

/// Background thread routine to handle standard streams.
///
/// Loop:
///     check for new channels
///     poll fds with mio (using a timeout)
///     read all readable fds until `WouldBlock`
///     deregister any fds that have closed
fn handler(channel: &cc::Receiver<Message>) {
    let mut connections = Slab::with_capacity(128);
    let poll = Poll::new().unwrap();
    let mut closed = false;
    while !closed {
        if let Some(message) = channel.try_recv() {
            match message {
                Message::Add(fd, stream) => {
                    if let Err(e) = poll.register(
                        &EventedFd(&fd),
                        Token(connections.insert(Connection::new(fd, stream))),
                        Ready::readable() | UnixReady::hup(),
                        PollOpt::edge(),
                    ) {
                        error!("Failed to register stream with mio ({})", e);
                    }
                }
                Message::Close => closed = true,
            }
        }

        let mut events = Events::with_capacity(128);
        if let Err(e) = poll.poll(&mut events, Some(Duration::from_millis(50))) {
            error!("Failed to poll mio ({}). Abandoning stream redirection.", e);
            break;
        }

        for event in &events {
            let Token(handle) = event.token();
            if event.readiness().is_readable() {
                let mut connection = connections.get_mut(handle).unwrap();
                loop {
                    match read_line(&mut connection.source) {
                        Ok(None) => break,                              // WouldBlock
                        Ok(Some(ref line)) if line.is_empty() => break, // imminent HUP
                        Ok(Some(line)) => {
                            match write_line(&connection.sink, &line[..line.len() - 1]) {
                                Ok(()) => (),
                                Err(e) => warn!("Stream redirection failure ({}): {}", e, line),
                            }
                        }
                        Err(e) => {
                            warn!("Stream error {}", e);
                            break;
                        }
                    }
                }
            }
            if UnixReady::from(event.readiness()).is_hup() {
                let mut connection = connections.remove(handle);
                poll.deregister(&EventedFd(&connection.source.into_inner().into_raw_fd()))
                    .unwrap();
            }
        }
    }
}

/// Reads a line from the BufReader.
///
/// Note: The underlying fd is already set O_NONBLOCK
///
/// Returns `Ok(Some(String))` on success
///         `Ok(None)` if `BufReader::read_line()` returns `WouldBlock`
///         `Err(io::Error()` otherwise
fn read_line(source: &mut BufReader<File>) -> io::Result<Option<String>> {
    let mut line = String::new();
    match source.read_line(&mut line) {
        Ok(_) => Ok(Some(line)),
        Err(e) => match e.kind() {
            io::ErrorKind::WouldBlock => Ok(None),
            _ => Err(e),
        },
    }
}

/// Writes a line to file or to syslog (TCP or UDP)
fn write_line(sink: &Stream, line: &str) -> io::Result<()> {
    match sink {
        Stream::File { filename } => {
            OpenOptions::new()
                .create_new(true)
                .append(true)
                .open(filename)?
                .write_all(line.as_ref())?;
            Ok(())
        }
        Stream::Syslog {
            address,
            facility,
            severity,
        } => {
            let formatter = Formatter3164 {
                facility: *facility,
                hostname: None,
                process: String::from("riffol"),
                pid: 0,
            };
            let mut logger: syslog::Result<
                Logger<LoggerBackend, String, Formatter3164>,
            > = match address {
                Address::Unix(address) => match address {
                    Some(address) => syslog::unix_custom(formatter, address),
                    None => syslog::unix(formatter),
                },
                Address::Tcp(server) => syslog::tcp(formatter, server),
                Address::Udp { server, local } => syslog::udp(formatter, local, server),
            };
            let line = line.to_owned();

            match logger {
                Ok(mut logger) => match severity {
                    x if *x == LOG_EMERG as u32 => logger.emerg(line),
                    x if *x == LOG_ALERT as u32 => logger.alert(line),
                    x if *x == LOG_CRIT as u32 => logger.crit(line),
                    x if *x == LOG_ERR as u32 => logger.err(line),
                    x if *x == LOG_WARNING as u32 => logger.warning(line),
                    x if *x == LOG_NOTICE as u32 => logger.notice(line),
                    x if *x == LOG_INFO as u32 => logger.info(line),
                    _ => logger.debug(line),
                }.map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{}", e))),
                _ => Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Couldn't create logging instance",
                )),
            }
        }
        #[cfg(test)]
        Stream::Stdout => Ok(println!("{}", line)),
    }
}

#[cfg(test)]
mod test {
    use super::Stream;
    use std::os::unix::io::IntoRawFd;
    use std::process::{Command, Stdio};

    #[test]
    fn test1() {
        let handler = super::Handler::new();
        let mut child1 = Command::new("ping")
            .arg("-c2")
            .arg("8.8.4.4")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let mut child2 = Command::new("ls")
            .arg("-l")
            .arg("/")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        handler.add_stream(child1.stdout.take().unwrap().into_raw_fd(), &Stream::Stdout);
        handler.add_stream(child1.stderr.take().unwrap().into_raw_fd(), &Stream::Stdout);
        handler.add_stream(child2.stdout.take().unwrap().into_raw_fd(), &Stream::Stdout);
        handler.add_stream(child2.stderr.take().unwrap().into_raw_fd(), &Stream::Stdout);

        child2.wait().unwrap();
        child1.wait().unwrap();
    }
}
