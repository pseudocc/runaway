use std::io;
use std::fs;
use std::path::{Path, PathBuf};

mod unix {
    pub(super) use std::os::unix::net::{
        UnixStream as Stream,
        UnixListener as Listener,
    };
}

#[derive(Debug)]
pub struct AppContext {
    socket: unix::Stream,
}

impl AppContext {
    fn new(socket: unix::Stream) -> Self {
        Self { socket }
    }

    fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            socket: self.socket.try_clone()?,
        })
    }
}

pub struct App {
    uid: u32,
    gid: u32,
    socket_path: PathBuf,

    // TODO: add real state variables here
    counter: usize,
}

impl App {
    pub fn new<P: AsRef<Path>>(socket_path: P) -> Self {
        App {
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            socket_path: socket_path.as_ref().to_path_buf(),
            counter: 0,
        }
    }

    pub fn on_connect(&mut self, ctx: &mut AppContext) -> io::Result<()> {
        use std::io::{Read, Write};
        println!("app: new connection from socket");
        let mut message = [0u8; 7];
        ctx.socket.read_exact(&mut message)?;
        match &message {
            b"counter" => {
                self.counter += 1;
                let response = format!("counter: {}", self.counter);
                ctx.socket.write_all(response.as_bytes())?;
                Ok(())
            },
            _ => {
                println!("app: received unknown message: {:?}", message);
                Err(io::Error::new(io::ErrorKind::InvalidData, "unknown message"))
            },
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

            let ready_items = match poll.wait() {
                Ok(items) => items,
                Err(e) => {
                    eprintln!("app: poll error: {}", e);
                    Vec::new()
                },
            };

            // TODO: pidfd for child processes
            let listener_ready = ready_items
                .iter()
                .any(|item| item.raw_fd() == listener.as_raw_fd());

            if listener_ready {
                loop {
                    let mut context = match listener.accept() {
                        Ok((socket, _)) => AppContext::new(socket),
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => {
                            eprintln!("app: accept error: {}", e);
                            continue;
                        },
                    };

                    if let Err(e) = self.on_connect(&mut context) {
                        eprintln!("app: error handling connection: {}", e);
                    }
                }
            }
        }
    }
}
