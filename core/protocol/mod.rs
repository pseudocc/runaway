use serde::{Serialize, Deserialize};
use std::result::Result;

pub mod app;

mod format {
    use super::*;

    #[derive(Debug)]
    pub enum Error {
        Bincode(bincode::Error),
        Json(serde_json::Error),
    }

    pub trait Control {
        fn deserialize<'de, T>(data: &'de [u8]) -> Result<T, Error>
        where
            T: Deserialize<'de>;

        fn serialize<T>(value: &T) -> Result<Vec<u8>, Error>
        where
            T: Serialize;
    }

    pub struct Json;
    impl Control for Json {
        fn deserialize<'de, T>(data: &'de [u8]) -> Result<T, Error>
        where
            T: Deserialize<'de>,
        {
            serde_json::from_slice(data).map_err(Error::Json)
        }

        fn serialize<T>(value: &T) -> Result<Vec<u8>, Error>
        where
            T: Serialize,
        {
            serde_json::to_vec(value).map_err(Error::Json)
        }
    }

    pub struct Bincode;
    impl Control for Bincode {
        fn deserialize<'de, T>(data: &'de [u8]) -> Result<T, Error>
        where
            T: Deserialize<'de>,
        {
            bincode::deserialize(data).map_err(Error::Bincode)
        }

        fn serialize<T>(value: &T) -> Result<Vec<u8>, Error>
        where
            T: Serialize,
        {
            bincode::serialize(value).map_err(Error::Bincode)
        }
    }
}

pub use format::{Json, Bincode};

mod end {
    use super::*;

    pub trait Control {
        type Error;

        fn send<T>(&mut self, value: &T) -> Result<(), Self::Error>
        where
            T: Serialize;

        fn receive<T>(&mut self) -> Result<T, Self::Error>
        where
            T: for<'de> Deserialize<'de>;
    }
}

pub mod typed {
    use serde::{Serialize, Deserialize};

    pub trait Request {
        type Request: Serialize;
        type Response: for<'de> Deserialize<'de>;
        type Output;
        type Error;

        fn into_request(self) -> Self::Request;
        fn from_response(response: Self::Response) -> Result<Self::Output, Self::Error>;
    }

    pub trait Handler {
        type Error;

        fn handle<R: Request<Error = Self::Error>>(&mut self, request: R) -> Result<R::Output, R::Error>;
    }

    impl<C: super::end::Control> Handler for C {
        type Error = C::Error;

        fn handle<R: Request<Error = C::Error>>(&mut self, request: R) -> Result<R::Output, R::Error> {
            let request = request.into_request();
            self.send(&request)?;
            let response = self.receive()?;
            R::from_response(response)
        }
    }
}
