use std::io;
use std::io::{Read, Write};
use std::fs::File;
use std::os::fd::{FromRawFd, RawFd};

use crate::utils::libc::wrap_err;

pub struct ReadEnd(File);
pub struct WriteEnd(File);

impl Read for ReadEnd {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl From<ReadEnd> for File {
    fn from(read_end: ReadEnd) -> Self {
        read_end.0
    }
}

impl Write for WriteEnd {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl From<WriteEnd> for File {
    fn from(write_end: WriteEnd) -> Self {
        write_end.0
    }
}

pub struct Pipe {
    read_end: ReadEnd,
    write_end: WriteEnd,
}

#[derive(Default)]
pub struct PipeOptions {
    non_blocking: bool,
    close_on_exec: bool,
}

impl PipeOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn non_blocking(mut self) -> Self {
        self.non_blocking = true;
        self
    }

    pub fn close_on_exec(mut self) -> Self {
        self.close_on_exec = true;
        self
    }

    pub fn open(self) -> io::Result<Pipe> {
        let mut fds: [RawFd; 2] = [-1, -1];
        let mut flags = 0;
        if self.non_blocking {
            flags |= libc::O_NONBLOCK;
        }
        if self.close_on_exec {
            flags |= libc::O_CLOEXEC;
        }
        _ = wrap_err!(libc::pipe2(fds.as_mut_ptr(), flags))?;
        unsafe {
            Ok(Pipe {
                read_end: ReadEnd(File::from_raw_fd(fds[0])),
                write_end: WriteEnd(File::from_raw_fd(fds[1])),
            })
        }
    }
}

impl Pipe {
    pub fn new() -> io::Result<Pipe> {
        PipeOptions {
            non_blocking: false,
            close_on_exec: true,
        }.open()
    }

    pub fn read(self) -> ReadEnd {
        self.read_end
    }

    pub fn write(self) -> WriteEnd {
        self.write_end
    }

    pub fn into_ends(self) -> (ReadEnd, WriteEnd) {
        (self.read_end, self.write_end)
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::io::{Read, Write};
    use super::{Pipe, PipeOptions, wrap_err};

    #[test]
    fn test_fork_pipe() -> io::Result<()> {
        let pipe = Pipe::new()?;
        match wrap_err!(libc::fork())? {
            0 => {
                let mut buffer = [0u8; 5];
                let mut read_end = pipe.read();
                read_end.read_exact(&mut buffer)?;
                assert_eq!(&buffer, b"Hello");
            },
            pid => {
                let mut write_end = pipe.write();
                write_end.write_all(b"Hello")?;
                unsafe {
                    libc::waitpid(pid, std::ptr::null_mut(), 0);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn test_regular_pipe() -> io::Result<()> {
        let pipe = PipeOptions::new()
            .non_blocking()
            .close_on_exec()
            .open()?;
        let mut buffer = [0u8; 5];
        let (mut read_end, mut write_end) = pipe.into_ends();

        let would_block = read_end.read(&mut buffer).unwrap_err();
        assert_eq!(would_block.kind(), io::ErrorKind::WouldBlock);

        write_end.write_all(b"World")?;
        read_end.read_exact(&mut buffer)?;
        assert_eq!(&buffer, b"World");

        Ok(())
    }
}
