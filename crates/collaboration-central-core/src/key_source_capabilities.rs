//! Public-key responses and fetch failures do not depend on an object runtime.
pub struct RawJwks {
    pub body: Vec<u8>,
    pub cache_control: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyFetchFailure {
    Unreachable,
    BadResponse { status: u16 },
}
