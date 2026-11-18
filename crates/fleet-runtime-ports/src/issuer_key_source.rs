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

/// A host binds this port to the issuer's key address from pinned configuration.
pub trait IssuerKeySource: Send + Sync {
    fn configured_source(&self) -> &str;
    fn fetch_issuer_keys(
        &self,
    ) -> impl std::future::Future<Output = Result<RawJwks, KeyFetchFailure>> + Send;
}
