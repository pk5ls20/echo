//! ref: <https://github.com/dtolnay/case-studies/blob/master/autoref-specialization/README.md>
use echo_macros::EchoBusinessError;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::Display;
use std::marker::PhantomData;
use std::str::FromStr;

#[derive(Debug, thiserror::Error, EchoBusinessError)]
pub enum SmartStringError {
    #[error(transparent)]
    SerdeJsonError(serde_json::Error),
    #[error("FromStr error: {0}")]
    FromStrError(String),
}
pub type SmartStringResult<T> = Result<T, SmartStringError>;

pub trait DisplayToString {
    fn smart_to_string(&self) -> SmartStringResult<String>;
}

impl<T: ToString> DisplayToString for T {
    fn smart_to_string(&self) -> SmartStringResult<String> {
        Ok(self.to_string())
    }
}

pub trait SerdeToString {
    fn smart_to_string(&self) -> SmartStringResult<String>;
}

impl<T: Serialize> SerdeToString for &T {
    fn smart_to_string(&self) -> SmartStringResult<String> {
        serde_json::to_string(self).map_err(SmartStringError::SerdeJsonError)
    }
}

pub struct ParseTarget<T>(PhantomData<fn() -> T>);

impl<T> ParseTarget<T> {
    pub const NEW: Self = ParseTarget(PhantomData);
}

pub trait SmartParse<T> {
    fn smart_parse(self, s: &str) -> SmartStringResult<T>;
}

impl<T> SmartParse<T> for ParseTarget<T>
where
    T: FromStr,
    <T as FromStr>::Err: Display,
{
    fn smart_parse(self, s: &str) -> SmartStringResult<T> {
        T::from_str(s).map_err(|e| SmartStringError::FromStrError(e.to_string()))
    }
}

impl<T> SmartParse<T> for &ParseTarget<T>
where
    T: DeserializeOwned,
{
    fn smart_parse(self, s: &str) -> SmartStringResult<T> {
        serde_json::from_str::<T>(s).map_err(SmartStringError::SerdeJsonError)
    }
}

/// Auxiliary trait for constraints, with the simple assumption that [`ToString`] ∈ [`serde::Serialize`]
pub trait SmartString: Serialize + DeserializeOwned {}
impl<T> SmartString for T where T: Serialize + DeserializeOwned {}

#[macro_export]
macro_rules! smart_string {
    ($e:expr) => {
        (&$e).smart_to_string()
    };
}

pub mod prelude {
    pub use super::{
        DisplayToString as _, SerdeToString as _, SmartParse, SmartString, SmartStringResult,
    };
}
