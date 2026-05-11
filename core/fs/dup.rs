use std::io;
use std::os::fd::AsRawFd;
use crate::utils::libc::wrap_err;

#[derive(Default)]
pub struct DupOptions {
    non_blocking: bool,
    close_on_exec: bool,
}

impl DupOptions {
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

    pub fn dup<O, N>(self, old: &O, new: &N) -> io::Result<()>
    where
        O: AsRawFd,
        N: AsRawFd,
    {
        let mut flags = 0;
        if self.non_blocking {
            flags |= libc::O_NONBLOCK;
        }
        if self.close_on_exec {
            flags |= libc::O_CLOEXEC;
        }
        _ = wrap_err!(libc::dup3(old.as_raw_fd(), new.as_raw_fd(), flags))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use crate::fs::DupOptions;

    #[test]
    fn test_redirect() {
        use io::Write;
        use std::fs;

        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open("/tmp/dup_test.txt")
            .unwrap();

        DupOptions::new()
            .dup(&file, &io::stdout())
            .unwrap();

        io::stdout().write_all(b"Hello, world!").unwrap();
        io::stdout().flush().unwrap();
        let contents = fs::read_to_string("/tmp/dup_test.txt").unwrap();
        assert_eq!(contents, "Hello, world!");
    }
}
