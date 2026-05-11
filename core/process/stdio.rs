use std::io;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, RawFd};
use crate::io::Pipe;
use crate::fs::DupOptions;

pub(crate) enum StdioKind {
    Stdin,
    Stdout,
    Stderr,
}

impl AsRawFd for StdioKind {
    fn as_raw_fd(&self) -> RawFd {
        match self {
            StdioKind::Stdin => libc::STDIN_FILENO,
            StdioKind::Stdout => libc::STDOUT_FILENO,
            StdioKind::Stderr => libc::STDERR_FILENO,
        }
    }
}

enum ChildStdio {
    Inherit,
    Null,
    MakePipe,
    Owned(File),
    Borrowed(RawFd),
}

pub(super) enum InterimStdio {
    Inherit,
    Null,
    Pipe(Pipe),
    Owned(File),
    Borrowed(RawFd),
}

enum WrapFile {
    Owned(File),
    Borrowed(RawFd),
}

impl AsRawFd for WrapFile {
    fn as_raw_fd(&self) -> RawFd {
        match self {
            WrapFile::Owned(f) => f.as_raw_fd(),
            WrapFile::Borrowed(fd) => *fd,
        }
    }
}

impl InterimStdio {
    pub(super) fn child(self, stdio: StdioKind) -> io::Result<()> {
        let is_stdin = match stdio {
            StdioKind::Stdin => true,
            _ => false,
        };

        let old_file: WrapFile = match self {
            Self::Inherit => {
                return Ok(());
            },
            Self::Null => {
                let dev_null = OpenOptions::new()
                    .read(is_stdin)
                    .write(!is_stdin)
                    .open("/dev/null")?;
                WrapFile::Owned(dev_null)
            },
            Self::Pipe(p) => {
                let (read, write) = p.into_ends();
                let end: File = if is_stdin {
                    read.into()
                } else {
                    write.into()
                };
                WrapFile::Owned(end)
            },
            Self::Owned(f) => WrapFile::Owned(f),
            Self::Borrowed(fd) => WrapFile::Borrowed(fd),
        };

        DupOptions::new()
            .close_on_exec()
            .dup(&old_file, &stdio)
    }

    pub(super) fn parent(self, stdio: StdioKind) -> Option<File> {
        match self {
            Self::Pipe(p) => {
                let (read, write) = p.into_ends();
                match stdio {
                    StdioKind::Stdin => Some(write.into()),
                    _ => Some(read.into()),
                }
            },
            Self::Owned(f) => Some(f),
            _ => None,
        }
    }
}

impl TryFrom<Stdio> for InterimStdio {
    type Error = io::Error;

    fn try_from(value: Stdio) -> Result<Self, Self::Error> {
        match value.0 {
            ChildStdio::Inherit => Ok(InterimStdio::Inherit),
            ChildStdio::Null => Ok(InterimStdio::Null),
            ChildStdio::MakePipe => Ok(InterimStdio::Pipe(Pipe::new()?)),
            ChildStdio::Owned(f) => Ok(InterimStdio::Owned(f)),
            ChildStdio::Borrowed(fd) => Ok(InterimStdio::Borrowed(fd)),
        }
    }
}

pub struct Stdio(ChildStdio);

impl Stdio {
    pub fn inherit() -> Self {
        Stdio(ChildStdio::Inherit)
    }

    pub fn null() -> Self {
        Stdio(ChildStdio::Null)
    }

    pub fn piped() -> Self {
        Stdio(ChildStdio::MakePipe)
    }

    pub fn borrowed<F: AsRawFd>(f: F) -> Self {
        Stdio(ChildStdio::Borrowed(f.as_raw_fd()))
    }
}

impl From<Stdio> for Option<File> {
    fn from(stdio: Stdio) -> Self {
        match stdio.0 {
            ChildStdio::Owned(f) => Some(f),
            _ => None,
        }
    }
}

impl From<File> for Stdio {
    fn from(file: File) -> Self {
        Stdio(ChildStdio::Owned(file))
    }
}

impl From<&File> for Stdio {
    fn from(file: &File) -> Self {
        Stdio(ChildStdio::Borrowed(file.as_raw_fd()))
    }
}
