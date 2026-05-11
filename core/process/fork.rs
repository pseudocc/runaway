use std::io;
use std::fs::File;
use std::os::fd::{FromRawFd, RawFd};
use crate::utils::libc::wrap_err;
use super::stdio::{Stdio, StdioKind, InterimStdio};
use super::common::Process;

pub struct Fork {
    stdin: Stdio,
    stdout: Stdio,
    stderr: Stdio,
}

impl Fork {
    pub fn new() -> Self {
        Fork {
            stdin: Stdio::inherit(),
            stdout: Stdio::inherit(),
            stderr: Stdio::inherit(),
        }
    }

    pub fn stdin<S>(mut self, stdio: S) -> Self
    where
        S: Into<Stdio>,
    {
        self.stdin = stdio.into();
        self
    }

    pub fn stdout<S>(mut self, stdio: S) -> Self
    where
        S: Into<Stdio>,
    {
        self.stdout = stdio.into();
        self
    }

    pub fn stderr<S>(mut self, stdio: S) -> Self
    where
        S: Into<Stdio>,
    {
        self.stderr = stdio.into();
        self
    }

    pub fn fork(self) -> io::Result<ForkResult> {
        let stdin = InterimStdio::try_from(self.stdin)?;
        let stdout = InterimStdio::try_from(self.stdout)?;
        let stderr = InterimStdio::try_from(self.stderr)?;

        match wrap_err!(libc::fork())? {
            0 => {
                stdin.child(StdioKind::Stdin)?;
                stdout.child(StdioKind::Stdout)?;
                stderr.child(StdioKind::Stderr)?;
                Ok(ForkResult::Child)
            },
            pid => {
                let pidfd = wrap_err!(libc::syscall(libc::SYS_pidfd_open, pid, 0) as RawFd)?;
                let pid_file = unsafe { File::from_raw_fd(pidfd) };
                Ok(ForkResult::Parent(Process {
                    pid,
                    pid_file,
                    stdin: stdin.parent(StdioKind::Stdin),
                    stdout: stdout.parent(StdioKind::Stdout),
                    stderr: stderr.parent(StdioKind::Stderr),
                }))
            },
        }
    }
}

pub enum ForkResult {
    Child,
    Parent(Process),
}

impl ForkResult {
    pub fn is_child(&self) -> bool {
        match self {
            Self::Child => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use crate::process::{Fork, ForkResult, Stdio};

    #[test]
    fn test_fork() {
        let fork = Fork::new()
            .stdout(Stdio::piped())
            .fork()
            .unwrap();
        match fork {
            ForkResult::Child => {
                let mut stdout = std::io::stdout();
                stdout.write_all(b"child\n").unwrap();
                stdout.flush().unwrap();
                std::process::exit(0);
            },
            ForkResult::Parent(mut process) => {
                let status = process.wait().unwrap();
                assert!(status.success());

                let mut output = String::new();
                let mut stdout = process.stdout.take().unwrap();
                stdout.read_to_string(&mut output).unwrap();
                assert_eq!(output, "child\n");
            },
        }
    }
}
