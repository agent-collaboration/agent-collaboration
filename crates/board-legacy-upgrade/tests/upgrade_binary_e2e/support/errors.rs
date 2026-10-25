use std::{error::Error, fmt::Display, io};

pub type E2eResult<Value = ()> = Result<Value, Box<dyn Error + Send + Sync>>;

pub fn promote_error(error: impl Display) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::other(error.to_string()))
}

pub fn promote_result<Value>(result: Result<Value, Box<dyn Error>>) -> E2eResult<Value> {
    result.map_err(promote_error)
}
