use std::io;
use std::fs;
use std::collections::HashMap;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use crate::protocol;

mod unix {
    pub(super) use std::os::unix::net::{
        UnixStream as Stream,
        UnixListener as Listener,
    };
}

pub mod typed {
    use crate::protocol;

    pub trait Request {
        type Output;

        fn into_request(self) -> protocol::Request;
        fn from_response(reponse: protocol::Response) -> protocol::Result<Self::Output>;
    }

    impl Request for protocol::CounterAction {
        type Output = usize;

        fn into_request(self) -> protocol::Request {
            return protocol::Request::CounterAction(self);
        }

        fn from_response(response: protocol::Response) -> protocol::Result<Self::Output> {
            match response {
                protocol::Response::CounterValue(n) => Ok(n),
                _ => return Err(protocol::Error::InvalidRequest),
            }
        }
    }
}

pub struct AppContext {
    handler: protocol::Client,
}

impl AppContext {
    fn call<R: typed::Request>(&mut self, request: R) -> protocol::Result<R::Output> {
        use crate::protocol::EndControl;
        let protocol_request = request.into_request();
        self.handler.send(&protocol_request)?;
        let protocol_response = self.handler.receive()?;
        R::from_response(protocol_response)
    }
}

pub struct App {
    uid: u32,
    gid: u32,
    socket_path: PathBuf,
    max_connections: usize,
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
            max_connections: 512,
            connections: HashMap::new(),
            counter: 0,
        }
    }

    fn fork_poc(&mut self) -> io::Result<()> {
        use protocol::*;
        use crate::process::{Fork, ForkResult};
        let (stream, child_stream) = unix::Stream::pair()?;

        match Fork::new().fork()? {
            ForkResult::Child => {
                use std::{thread, time::Duration};
                let handler = protocol::Client::new(child_stream)?;
                let mut app_context = AppContext { handler };
                for counter_actions in [
                    CounterAction::Increment,
                    CounterAction::Increment,
                    CounterAction::Get,
                    CounterAction::Decrement,
                ] {
                    let counter_value = app_context.call(counter_actions)?;
                    println!("child: counter={}", counter_value);
                    thread::sleep(Duration::from_secs(1));
                }

                std::process::exit(0);
            },
            ForkResult::Parent(process) => {
                println!("app: forked child process with pid {}", process.pid);
                self.on_connect(stream)?;
            },
        }
        Ok(())
    }

    fn on_connect(&mut self, stream: unix::Stream) -> protocol::Result<()> {
        use crate::protocol::*;
        use std::os::unix::io::AsRawFd;

        println!("app: new connection from socket");
        let mut stream = stream;
        let fd = stream.as_raw_fd();
        let mut handler = Server::new(&mut stream, true);
        let request = handler.receive()?;
        match request {
            Request::ClientId if self.connections.len() >= self.max_connections =>
                handler.send_error(protocol::Error::ServerBusy),
            Request::ClientId => {
                let response = Response::ClientId(handler.client_id);
                handler.send(&response)?;
                self.connections.insert(fd, stream);
                Ok(())
            },
            _ => handler.send_error(protocol::Error::InvalidRequest),
        }
    }

    fn handle_request(&mut self, fd: RawFd) -> protocol::Result<()> {
        use protocol::*;

        let mut stream = match self.connections.get_mut(&fd) {
            Some(s) => s,
            None => unreachable!("app: handle_request called with unknown fd {}", fd),
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
            _ => handler.send_error(protocol::Error::InvalidRequest),
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
        let mut test_fork_poc = true;

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

            if test_fork_poc {
                if let Err(e) = self.fork_poc() {
                    eprintln!("app: error in fork poc: {}", e);
                }
                test_fork_poc = false;
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
