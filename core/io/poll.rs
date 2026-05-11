use std::io;
use std::fs::File;
use std::os::fd::{AsRawFd, RawFd};
use crate::utils::libc::wrap_err;

pub struct PollItem<'f> {
    fd: PollFd<'f>,
    read: bool,
    write: bool,
    error: bool,
    hangup: bool,
}

enum PollFd<'f> {
    Borrowed(&'f File),
    Raw(RawFd),
}

impl<'f> PollFd<'f> {
    fn raw_fd(&self) -> RawFd {
        match self {
            PollFd::Borrowed(f) => f.as_raw_fd(),
            PollFd::Raw(fd) => *fd,
        }
    }
}

impl<'f> From<&PollItem<'f>> for libc::pollfd {
    fn from(item: &PollItem<'f>) -> Self {
        let mut events = 0;
        if item.read {
            events |= libc::POLLIN;
        }
        if item.write {
            events |= libc::POLLOUT;
        }
        libc::pollfd {
            fd: item.fd.raw_fd(),
            events,
            revents: 0,
        }
    }
}

impl<'f> PollItem<'f> {
    pub fn new(file: &'f File) -> Self {
        PollItem {
            fd: PollFd::Borrowed(file),
            read: true,
            write: false,
            error: false,
            hangup: false,
        }
    }

    /// Create a PollItem from any type that implements AsRawFd.
    /// The caller must ensure the fd outlives this PollItem.
    pub fn from_fd(fd: &'f impl AsRawFd) -> Self {
        PollItem {
            fd: PollFd::Raw(fd.as_raw_fd()),
            read: true,
            write: false,
            error: false,
            hangup: false,
        }
    }

    pub fn read(mut self, enable: bool) -> Self {
        self.read = enable;
        self
    }

    pub fn write(mut self, enable: bool) -> Self {
        self.write = enable;
        self
    }

    pub fn can_read(&self) -> io::Result<bool> {
        if self.error {
            return Err(io::Error::new(io::ErrorKind::Other, "poll error on fd"));
        }
        Ok(self.read)
    }

    pub fn can_write(&self) -> io::Result<bool> {
        if self.error {
            return Err(io::Error::new(io::ErrorKind::Other, "poll error on fd"));
        }
        if self.hangup {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "poll hangup on fd"));
        }
        Ok(self.write)
    }

    pub fn raw_fd(&self) -> RawFd {
        self.fd.raw_fd()
    }
}

#[derive(Clone, Copy)]
pub enum PollTimeout {
    Infi,
    Msec(u32),
}

impl Into<i32> for PollTimeout {
    fn into(self) -> i32 {
        match self {
            PollTimeout::Infi => -1,
            PollTimeout::Msec(ms) => i32::try_from(ms).unwrap_or(i32::MAX),
        }
    }
}

pub struct Poll<'f> {
    items: Vec<PollItem<'f>>,
    timeout: PollTimeout,
}

impl<'f> Poll<'f> {
    pub fn new() -> Self {
        Poll {
            items: Vec::new(),
            timeout: PollTimeout::Infi,
        }
    }

    pub fn add(&mut self, item: PollItem<'f>) -> &mut Self {
        self.items.push(item);
        self
    }

    pub fn timeout(&mut self, timeout: PollTimeout) -> &mut Self {
        self.timeout = timeout;
        self
    }

    pub fn wait(self) -> io::Result<Vec<PollItem<'f>>> {
        let mut poll_fds: Vec<libc::pollfd> = self.items.iter().map(|item| item.into()).collect();
        let n = wrap_err!(libc::poll(
            poll_fds.as_mut_ptr(),
            poll_fds.len() as libc::nfds_t,
            self.timeout.into()
        ))? as usize;
        if n == 0 {
            return Ok(Vec::new());
        }
        let mut ready_items = Vec::with_capacity(n);
        for (i, pfd) in poll_fds.iter().enumerate() {
            if pfd.revents != 0 {
                let item = &self.items[i];
                let fd = match &item.fd {
                    PollFd::Borrowed(f) => PollFd::Borrowed(f),
                    PollFd::Raw(fd) => PollFd::Raw(*fd),
                };
                let ready_item = PollItem {
                    fd,
                    read: (pfd.revents & libc::POLLIN) != 0,
                    write: (pfd.revents & libc::POLLOUT) != 0,
                    error: (pfd.revents & libc::POLLERR) != 0,
                    hangup: (pfd.revents & libc::POLLHUP) != 0,
                };
                ready_items.push(ready_item);
            }
        }
        Ok(ready_items)
    }
}
