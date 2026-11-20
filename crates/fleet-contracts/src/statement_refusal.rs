//! Closed failures shared by stamp decoding and verified identity admission.
use crate::IssuerSignatureRefusal;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FleetOrganizationStatementRefusal {
    #[error("Fleet organization statement is malformed")]
    Malformed,
    #[error("Fleet organization statement has the wrong token type")]
    WrongTokenType,
    #[error("Fleet organization statement algorithm is unsupported")]
    UnsupportedAlgorithm,
    #[error("Fleet organization statement signature is invalid")]
    BadSignature,
    #[error("Fleet organization statement has the wrong purpose")]
    WrongPurpose,
    #[error("Fleet organization statement version is unsupported")]
    UnsupportedVersion,
    #[error("Fleet organization statement has the wrong issuer")]
    WrongIssuer,
    #[error("Fleet organization statement has the wrong audience")]
    WrongAudience,
    #[error("Fleet organization statement names a different organization")]
    OrganizationMismatch,
    #[error("Fleet organization statement outlives the 120-day policy")]
    LifetimeExceedsPolicy,
    #[error("Fleet organization statement has expired")]
    Expired,
    #[error("Fleet organization statement is not yet valid")]
    NotYetValid,
    #[error("Fleet organization statement trust configuration is invalid")]
    InvalidConfiguration,
    #[error("Fleet organization statement verification is unavailable: {0}")]
    VerificationUnavailable(IssuerSignatureRefusal),
}
