use std::io;
use serde::{Serialize, Deserialize};
use std::os::unix::net::UnixStream;
use std::marker::PhantomData;

use super::format;
pub use super::end::Control as EndControl;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClientId(pub i32);

impl ClientId {
    pub fn sentinel() -> Self {
        ClientId(-1)
    }
}

#[derive(Serialize, Deserialize)]
pub enum CounterAction {
    Increment,
    Decrement,
    Get,
}

#[derive(Serialize, Deserialize)]
pub struct TrekId {
    way: String,
    number: usize,
}

#[derive(Serialize, Deserialize)]
pub enum TrekStatus {
    InProgress,
    Completed,
    Failed,
}

#[derive(Serialize, Deserialize)]
pub struct Embark {
    library: String,
    way: String,
    input: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub enum Request {
    ClientId,
    CounterAction(CounterAction),
    Embark(Embark),
    Status(TrekId),
}

#[derive(Serialize, Deserialize)]
pub enum Response {
    ClientId(ClientId),
    CounterValue(usize),
    ProtocolError(String),
    Embark(TrekId),
    Status(TrekStatus),
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Serde(format::Error),
    InvalidRequest,
    ClientIdMismatch(ClientId),
    ServerBusy,
    SizeLimit,
    Anyhow(String),
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<Error> for io::Error {
    fn from(err: Error) -> Self {
        match err {
            Error::Io(e) => e,
            Error::InvalidRequest => io::Error::new(io::ErrorKind::InvalidData, "invalid request"),
            Error::ClientIdMismatch(id) => io::Error::new(io::ErrorKind::ConnectionRefused, format!("client id {} does not match expected id", id.0)),
            Error::ServerBusy => io::Error::new(io::ErrorKind::ConnectionRefused, "server is busy"),
            Error::SizeLimit => io::Error::new(io::ErrorKind::InvalidData, "size limit exceeded"),
            Error::Serde(e)  => match e {
                format::Error::Bincode(e) => io::Error::new(io::ErrorKind::InvalidData, format!("bincode error: {}", e)),
                format::Error::Json(e) => io::Error::new(io::ErrorKind::InvalidData, format!("json error: {}", e)),
            },
            Error::Anyhow(msg) => io::Error::new(io::ErrorKind::Other, msg),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io::Error: {}", e),
            Self::Serde(e) => match e {
                format::Error::Bincode(e) => write!(f, "bincode error: {}", e),
                format::Error::Json(e) => write!(f, "json error: {}", e),
            },
            Self::InvalidRequest => write!(f, "invalid request"),
            Self::ClientIdMismatch(id) => write!(f, "client id {} does not match expected id", id.0),
            Self::ServerBusy => write!(f, "server is busy"),
            Self::SizeLimit => write!(f, "size limit exceeded"),
            Self::Anyhow(msg) => write!(f, "{}", msg),
        }
    }
}

pub mod impls {
    use super::*;

    pub struct Client<F: format::Control> {
        id: ClientId,
        stream: UnixStream,
        _marker: PhantomData<F>,
    }

    impl<F: format::Control> Client<F> {
        pub fn new(stream: UnixStream) -> Result<Self> {
            let mut client = Client {
                id: ClientId::sentinel(),
                stream,
                _marker: PhantomData,
            };
            client.send(&Request::ClientId)?;
            match client.receive()? {
                Response::ClientId(id) => {
                    client.id = id;
                    Ok(client)
                }
                _ => Err(Error::InvalidRequest),
            }
        }
    }

    impl<F: format::Control> EndControl for Client<F> {
        type Error = Error;
        fn send<T>(&mut self, value: &T) -> Result<()>
        where
            T: Serialize,
        {
            use std::io::Write;
            let id = self.id.0.to_le_bytes();
            let data = F::serialize(value).map_err(Error::Serde)?;
            let len = (data.len() + id.len()) as u32;
            self.stream.write_all(&len.to_be_bytes())?;
            self.stream.write_all(&id)?;
            self.stream.write_all(&data)?;
            self.stream.flush()?;
            Ok(())
        }

        fn receive<T>(&mut self) -> Result<T>
        where
            T: for<'de> Deserialize<'de>,
        {
            use std::io::Read;
            let mut len_buf = [0u8; 4];
            self.stream.read_exact(&mut len_buf)?;
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            self.stream.read_exact(&mut buf)?;
            F::deserialize(&buf).map_err(Error::Serde)
        }
    }

    pub struct Server<'so, F: format::Control> {
        pub(crate) client_id: ClientId,
        stream: &'so mut UnixStream,
        new_connection: bool,
        _marker: PhantomData<F>,
    }

    impl<'so, F: format::Control> Server<'so, F> {
        pub fn new(stream: &'so mut UnixStream, new_connection: bool) -> Self {
            use std::os::unix::io::AsRawFd;
            Server {
                client_id: ClientId(stream.as_raw_fd()),
                stream,
                new_connection,
                _marker: PhantomData,
            }
        }

        pub fn send_error(&mut self, err: Error) -> Result<()> {
            let response = Response::ProtocolError(err.to_string());
            // ignore any errors
            let _ = self.send(&response);
            Err(err)
        }
    }

    impl<'so, F: format::Control> EndControl for Server<'so, F> {
        type Error = Error;
        fn send<T>(&mut self, value: &T) -> Result<()>
        where
            T: Serialize,
        {
            use std::io::Write;
            let data = F::serialize(value).map_err(Error::Serde)?;
            let len = (data.len() as u32).to_be_bytes();
            self.stream.write_all(&len)?;
            self.stream.write_all(&data)?;
            self.stream.flush()?;
            Ok(())
        }

        fn receive<T>(&mut self) -> Result<T>
        where
            T: for<'de> Deserialize<'de>,
        {
            use std::io::Read;
            let mut len_buf = [0u8; 4];
            self.stream.read_exact(&mut len_buf)?;
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            self.stream.read_exact(&mut buf)?;
            if !self.new_connection {
                let id = ClientId(i32::from_le_bytes(buf[0..4].try_into().unwrap()));
                if id != self.client_id {
                    return Err(Error::ClientIdMismatch(id));
                }
            }

            F::deserialize(&buf[4..]).map_err(Error::Serde)
        }
    }
}

pub type Client = impls::Client<format::Bincode>;
pub type Server<'so> = impls::Server<'so, format::Bincode>;
