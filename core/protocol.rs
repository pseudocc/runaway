use std::io;
use serde::{Serialize, Deserialize};
use std::os::unix::net::UnixStream;

mod format {
    use super::*;
    pub trait Control {
        fn deserialize<'de, T>(data: &'de [u8]) -> io::Result<T>
        where
            T: Deserialize<'de>;

        fn serialize<T>(value: &T) -> io::Result<Vec<u8>>
        where
            T: Serialize;
    }

    pub struct Json;
    impl Control for Json {
        fn deserialize<'de, T>(data: &'de [u8]) -> io::Result<T>
        where
            T: Deserialize<'de>,
        {
            serde_json::from_slice(data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }

        fn serialize<T>(value: &T) -> io::Result<Vec<u8>>
        where
            T: Serialize,
        {
            serde_json::to_vec(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }
    }

    pub struct Bincode;
    impl Control for Bincode {
        fn deserialize<'de, T>(data: &'de [u8]) -> io::Result<T>
        where
            T: Deserialize<'de>,
        {
            bincode::deserialize(data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }

        fn serialize<T>(value: &T) -> io::Result<Vec<u8>>
        where
            T: Serialize,
        {
            bincode::serialize(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }
    }
}

pub use format::{Json, Bincode};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
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
pub enum Request {
    ClientId,
    CounterAction(CounterAction),
}

#[derive(Serialize, Deserialize)]
pub enum ProtocolError {
    MaxConnectionReached,
    InvalidRequest,
}

#[derive(Serialize, Deserialize)]
pub enum Response {
    ClientId(ClientId),
    CounterValue(usize),
    ProtocolError(ProtocolError),
}

mod end {
    use super::*;
    use io::{Read, Write};
    use std::marker::PhantomData;

    pub trait Control<'sock> {
        fn send<T>(&mut self, value: &T) -> io::Result<()>
        where
            T: Serialize;

        fn receive<T>(&mut self) -> io::Result<T>
        where
            T: for<'de> Deserialize<'de>;
    }

    pub struct Any<F: format::Control> {
        socket: UnixStream,
        _marker: PhantomData<F>,
    }

    impl<F: format::Control> Any<F> {
        pub fn new(socket: UnixStream) -> Self {
            Any {
                socket,
                _marker: PhantomData,
            }
        }
    }

    impl<F: format::Control> Control<'_> for Any<F> {
        fn send<T>(&mut self, value: &T) -> io::Result<()>
        where
            T: Serialize,
        {
            let data = F::serialize(value)?;
            let len = (data.len() as u32).to_be_bytes();
            self.socket.write_all(&len)?;
            self.socket.write_all(&data)?;
            self.socket.flush()?;
            Ok(())
        }

        fn receive<T>(&mut self) -> io::Result<T>
        where
            T: for<'de> Deserialize<'de>,
        {
            let mut len_buf = [0u8; 4];
            self.socket.read_exact(&mut len_buf)?;
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            self.socket.read_exact(&mut buf)?;
            F::deserialize(&buf)
        }
    }

    pub struct Client<'sock, F: format::Control> {
        id: ClientId,
        socket: &'sock mut UnixStream,
        _marker: PhantomData<F>,
    }

    impl<'sock, F: format::Control> Client<'sock, F> {
        pub fn new(socket: &'sock mut UnixStream) -> io::Result<Self> {
            let mut client = Client {
                id: ClientId::sentinel(),
                socket,
                _marker: PhantomData,
            };
            client.send(&Request::ClientId)?;
            match client.receive()? {
                Response::ClientId(id) => {
                    client.id = id;
                    Ok(client)
                }
                _ => Err(io::Error::new(io::ErrorKind::InvalidData, "expected ClientId response")),
            }
        }
    }

    impl<'sock, F: format::Control> Control<'sock> for Client<'sock, F> {
        fn send<T>(&mut self, value: &T) -> io::Result<()>
        where
            T: Serialize,
        {
            let id = self.id.0.to_le_bytes();
            let data = F::serialize(value)?;
            let len = (data.len() + id.len()) as u32;
            self.socket.write_all(&len.to_be_bytes())?;
            self.socket.write_all(&id)?;
            self.socket.write_all(&data)?;
            self.socket.flush()?;
            Ok(())
        }

        fn receive<T>(&mut self) -> io::Result<T>
        where
            T: for<'de> Deserialize<'de>,
        {
            let mut len_buf = [0u8; 4];
            self.socket.read_exact(&mut len_buf)?;
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            self.socket.read_exact(&mut buf)?;
            F::deserialize(&buf)
        }
    }

    pub struct Server<'sock, F: format::Control> {
        pub(crate) client_id: ClientId,
        socket: &'sock mut UnixStream,
        new_connection: bool,
        _marker: PhantomData<F>,
    }

    impl<'sock, F: format::Control> Server<'sock, F> {
        pub fn new(socket: &'sock mut UnixStream, new_connection: bool) -> Self {
            use std::os::unix::io::AsRawFd;
            Server {
                client_id: ClientId(socket.as_raw_fd()),
                socket,
                new_connection,
                _marker: PhantomData,
            }
        }
    }

    impl<'sock, F: format::Control> Control<'sock> for Server<'sock, F> {
        fn send<T>(&mut self, value: &T) -> io::Result<()>
        where
            T: Serialize,
        {
            let data = F::serialize(value)?;
            let len = (data.len() as u32).to_be_bytes();
            self.socket.write_all(&len)?;
            self.socket.write_all(&data)?;
            self.socket.flush()?;
            Ok(())
        }

        fn receive<T>(&mut self) -> io::Result<T>
        where
            T: for<'de> Deserialize<'de>,
        {
            let mut len_buf = [0u8; 4];
            self.socket.read_exact(&mut len_buf)?;
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            self.socket.read_exact(&mut buf)?;
            if !self.new_connection {
                let id = ClientId(i32::from_le_bytes(buf[0..4].try_into().unwrap()));
                if id != self.client_id {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "client ID mismatch"));
                }
            }

            F::deserialize(&buf[4..])
        }
    }
}

pub use end::{Control as EndControl, Any};
pub type Client<'sock> = end::Client<'sock, Bincode>;
pub type Server<'sock> = end::Server<'sock, Bincode>;
