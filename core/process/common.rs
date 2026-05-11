use std::io;
use std::fs::File;
use crate::io::PollItem;
use crate::utils::libc::wrap_err;
use libc::pid_t as Pid;

pub struct Process {
    pub(crate) pid: Pid,
    pub pid_file: File,
    pub stdin: Option<File>,
    pub stdout: Option<File>,
    pub stderr: Option<File>,
}

impl<'f> From<&'f Process> for PollItem<'f> {
    fn from(process: &'f Process) -> Self {
        PollItem::from_fd(&process.pid_file)
    }
}

pub struct ExitStatus(libc::c_int);

#[derive(Debug)]
pub struct ExitStatusError(pub libc::c_int);

impl std::error::Error for ExitStatusError {}

impl std::fmt::Display for ExitStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if libc::WIFEXITED(self.0) {
            write!(f, "process exited with status {}", libc::WEXITSTATUS(self.0))
        } else if libc::WIFSIGNALED(self.0) {
            write!(f, "process terminated by signal {}", libc::WTERMSIG(self.0))
        } else if libc::WIFSTOPPED(self.0) {
            write!(f, "process stopped by signal {}", libc::WSTOPSIG(self.0))
        } else if libc::WIFCONTINUED(self.0) {
            write!(f, "process continued")
        } else {
            write!(f, "unknown process state: {}", self.0)
        }
    }
}

impl ExitStatus {
    pub fn success(&self) -> bool {
        libc::WIFEXITED(self.0) && libc::WEXITSTATUS(self.0) == 0
    }

    pub fn exit_ok(&self) -> Result<(), ExitStatusError> {
        if self.success() {
            Ok(())
        } else {
            Err(ExitStatusError(self.0))
        }
    }

    pub fn code(&self) -> Option<i32> {
        libc::WIFEXITED(self.0).then(|| libc::WEXITSTATUS(self.0))
    }

    pub fn signal(&self) -> Option<i32> {
        libc::WIFSIGNALED(self.0).then(|| libc::WTERMSIG(self.0))
    }

    pub fn core_dumped(&self) -> bool {
        libc::WIFSIGNALED(self.0) && libc::WCOREDUMP(self.0)
    }

    pub fn stopped_signal(&self) -> Option<i32> {
        libc::WIFSTOPPED(self.0).then(|| libc::WSTOPSIG(self.0))
    }

    pub fn continued(&self) -> bool {
        libc::WIFCONTINUED(self.0)
    }

    pub fn into_raw(&self) -> i32 {
        self.0
    }
}

impl Process {
    pub fn pid(&self) -> Pid {
        self.pid
    }

    pub fn signal(&self, signal: i32) -> io::Result<()> {
        _ = wrap_err!(libc::kill(self.pid, signal))?;
        Ok(())
    }

    pub fn kill(&self) -> io::Result<()> {
        self.signal(libc::SIGKILL)
    }

    pub fn wait(&self) -> io::Result<ExitStatus> {
        let mut status: libc::c_int = 0;
        _ = wrap_err!(libc::waitpid(self.pid, &mut status as *mut libc::c_int, 0))?;
        Ok(ExitStatus(status))
    }

    pub fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        let mut status: libc::c_int = 0;
        let r = wrap_err!(libc::waitpid(self.pid, &mut status as *mut libc::c_int, libc::WNOHANG))?;
        if r == 0 {
            Ok(None)
        } else {
            Ok(Some(ExitStatus(status)))
        }
    }
}
