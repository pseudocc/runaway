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

    pub trait Control<'so> {
        type Error; 

        fn send<T>(&mut self, value: &T) -> Result<(), Self::Error>
        where
            T: Serialize;

        fn receive<T>(&mut self) -> Result<T, Self::Error>
        where
            T: for<'de> Deserialize<'de>;
    }
}
