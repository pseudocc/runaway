macro_rules! wrap_err {
    ($expr:expr) => {
        unsafe {
            match $expr {
                -1 => Err(std::io::Error::last_os_error()),
                value => Ok(value),
            }
        }
    };
}

pub mod libc {
    pub(crate) use wrap_err;
}
