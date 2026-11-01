//! Public key response values shared by the independent verification paths.

pub struct RawJwks {
    pub body: Vec<u8>,
    pub cache_control: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyFetchFailure {
    Unreachable,
    BadResponse { status: u16 },
}
