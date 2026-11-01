//! Validated endpoint and native session references shared by service and clients.
use serde::{Deserialize, Serialize};

/// Invalid public identity, without including caller-supplied content.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("invalid {0}")]
    Invalid(&'static str),
    #[error(transparent)]
    LimitExceeded(#[from] crate::limits::LimitExceeded),
}

/// Canonical lowercase UUID identity; not a process ID or network locator.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct UuidIdentity(String);

impl TryFrom<String> for UuidIdentity {
    type Error = IdentityError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let valid = value.len() == 36
            && value.bytes().enumerate().all(|(index, byte)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                }
            });
        if valid {
            Ok(Self(value))
        } else {
            Err(IdentityError::Invalid("UUID"))
        }
    }
}
impl From<UuidIdentity> for String {
    fn from(value: UuidIdentity) -> Self {
        value.0
    }
}

/// Stable name for a named adapter endpoint within one Router service.
///
/// An endpoint identifies the adapter that interprets a [`SessionId`]. It is
/// not a computer, process, transport connection, or conversation identity.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EndpointId(String);
impl TryFrom<String> for EndpointId {
    type Error = IdentityError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        crate::limits::LimitName::EndpointIdMaxBytes.validate(value.len() as u64)?;
        let valid = value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if valid {
            Ok(Self(value))
        } else {
            Err(IdentityError::Invalid("endpoint ID"))
        }
    }
}
impl From<EndpointId> for String {
    fn from(value: EndpointId) -> Self {
        value.0
    }
}

/// Bounded opaque session identifier, interpreted only by its endpoint.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SessionId(String);
impl TryFrom<String> for SessionId {
    type Error = IdentityError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        crate::limits::LimitName::SessionIdMaxBytes.validate(value.len() as u64)?;
        if !value.contains('\0') {
            Ok(Self(value))
        } else {
            Err(IdentityError::Invalid("session ID"))
        }
    }
}
impl From<SessionId> for String {
    fn from(value: SessionId) -> Self {
        value.0
    }
}

/// An adapter endpoint within one Router service, independent of its locator.
///
/// `service_id` identifies the Router service; `endpoint_id` selects the
/// stable named adapter endpoint within that service.
#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EndpointRef {
    pub service_id: UuidIdentity,
    pub endpoint_id: EndpointId,
}

/// A conversation scoped to the endpoint that interprets its opaque ID.
///
/// The `session_id` value is owned and interpreted by the referenced endpoint;
/// it is not a Router-wide, computer-wide, process, or connection identity.
#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRef {
    pub endpoint: EndpointRef,
    pub session_id: SessionId,
}
