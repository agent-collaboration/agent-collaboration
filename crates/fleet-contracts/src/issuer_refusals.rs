//! Closed refusals from issuer key resolution and signature checks.
use fleet_runtime_ports::{KeyFetchFailure, VerifyFailure};

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IssuerKeyRefusal {
    #[error("the issuer's public key set is malformed")]
    MalformedKeySet,
    #[error("the issuer's signing key is unknown")]
    UnknownKey,
    #[error("the issuer key cache is unavailable")]
    CacheUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IssuerKeyResolutionRefusal {
    #[error("the issuer's key source could not be reached")]
    Fetch { reason: KeyFetchFailure },
    #[error(transparent)]
    KeySet(#[from] IssuerKeyRefusal),
    #[error("the receiver's key-cache clock is unavailable")]
    InvalidClock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IssuerSignatureRefusal {
    #[error(transparent)]
    Keys(#[from] IssuerKeyResolutionRefusal),
    #[error("the issuer-signature capability is unavailable")]
    Crypto { reason: VerifyFailure },
}
