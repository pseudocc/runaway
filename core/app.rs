use std::io;
use std::fs;
use std::collections::HashMap;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};

mod unix {
    pub(super) use std::os::unix::net::{
        UnixStream as Stream,
        UnixListener as Listener,
    };
}

#[derive(Debug)]
pub struct AppContext {
    stream: unix::Stream,
}

impl AppContext {
    fn new(stream: unix::Stream) -> Self {
        Self { stream }
    }
}

pub struct App {
    uid: u32,
    gid: u32,
    socket_path: PathBuf,
    connections: HashMap<RawFd, unix::Stream>,

    // TODO: add real state variables here
    counter: usize,
}

impl App {
    pub fn new<P: AsRef<Path>>(socket_path: P) -> Self {
        App {
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            socket_path: socket_path.as_ref().to_path_buf(),
            connections: HashMap::new(),
            counter: 0,
        }
    }

    pub fn on_connect(&mut self, stream: unix::Stream) -> io::Result<()> {
        use crate::protocol::*;
        use std::os::unix::io::AsRawFd;

        const MAX_CONNECTIONS: usize = 64;

        println!("app: new connection from socket");
        let mut stream = stream;
        let fd = stream.as_raw_fd();
        let mut handler = Server::new(&mut stream, true);
        let request = handler.receive()?;
        match request {
            Request::ClientId if self.connections.len() >= MAX_CONNECTIONS => {
                let response = Response::ProtocolError(ProtocolError::MaxConnectionReached);
                handler.send(&response)
            },
            Request::ClientId => {
                let response = Response::ClientId(handler.client_id);
                handler.send(&response)?;
                self.connections.insert(fd, stream);
                Ok(())
            },
            _ => {
                let response = Response::ProtocolError(ProtocolError::InvalidRequest);
                handler.send(&response)
            },
        }
    }

    pub fn handle_request(&mut self, fd: RawFd) -> io::Result<()> {
        use crate::protocol::*;

        let mut stream = match self.connections.get_mut(&fd) {
            Some(s) => s,
            None => return Err(io::Error::new(io::ErrorKind::NotFound, "connection not found")),
        };

        let mut handler = Server::new(&mut stream, false);
        let request = handler.receive()?;
        match request {
            Request::CounterAction(action) => {
                match action {
                    CounterAction::Increment => self.counter = self.counter.saturating_add(1),
                    CounterAction::Decrement => self.counter = self.counter.saturating_sub(1),
                    CounterAction::Get => (),
                };
                let response = Response::CounterValue(self.counter);
                handler.send(&response)
            },
            _ => {
                let response = Response::ProtocolError(ProtocolError::InvalidRequest);
                handler.send(&response)
            }
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        match fs::remove_file(&self.socket_path) {
            Ok(()) => {
                println!("app: removed existing socket file at {}", self.socket_path.display());
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }

        let listener = unix::Listener::bind(&self.socket_path)?;
        listener.set_nonblocking(true)?;

        loop {
            use crate::io::{Poll, PollItem, PollTimeout};
            use std::os::fd::AsRawFd;

            let mut poll = Poll::new();
            poll.timeout(PollTimeout::Msec(200));
            poll.add(PollItem::from_fd(&listener));

            for (fd, _) in &self.connections {
                poll.add(PollItem::from_fd(fd));
            }

            let ready_items = match poll.wait() {
                Ok(items) => items,
                Err(e) => {
                    eprintln!("app: poll error: {}", e);
                    Vec::new()
                },
            };

            // TODO: pidfd for child processes
            let mut listener_ready = false;
            let mut hangup_fds = Vec::new();
            let mut ready_fds = Vec::new();

            for item in ready_items {
                if item.raw_fd() == listener.as_raw_fd() {
                    if item.has_hangup() {
                        eprintln!("app: listener hangup");
                        return Err(io::Error::new(io::ErrorKind::Other, "listener hangup"));
                    }
                    listener_ready = item.can_read().unwrap_or(false);
                    continue;
                }

                if item.has_hangup() {
                    eprintln!("app: connection hangup on fd {}", item.raw_fd());
                    hangup_fds.push(item.raw_fd());
                    continue;
                }

                if item.can_read().unwrap_or(false) {
                    ready_fds.push(item.raw_fd());
                }
            }

            for fd in hangup_fds {
                self.connections.remove(&fd);
            }

            for fd in ready_fds {
                if let Err(e) = self.handle_request(fd) {
                    eprintln!("app: error handling request on fd {}: {}", fd, e);
                    self.connections.remove(&fd);
                }
            }

            if listener_ready {
                loop {
                    let stream = match listener.accept() {
                        Ok((s, _)) => s,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => {
                            eprintln!("app: accept error: {}", e);
                            continue;
                        },
                    };

                    if let Err(e) = self.on_connect(stream) {
                        eprintln!("app: error handling connection: {}", e);
                    }
                }
            }
        }
    }
}
