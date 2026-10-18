use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use uuid::{Uuid, Variant};

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{kind} must be a canonical lowercase RFC UUIDv7")]
pub struct IdentityError {
    kind: &'static str,
}

fn parse_uuid_v7(value: String, kind: &'static str) -> Result<String, IdentityError> {
    let bytes = value.as_bytes();
    let canonical_syntax = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)
            }
        })
        && bytes.get(14) == Some(&b'7')
        && matches!(bytes.get(19), Some(b'8' | b'9' | b'a' | b'b'));
    if !canonical_syntax {
        return Err(IdentityError { kind });
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let parsed = Uuid::parse_str(&value).map_err(|_| IdentityError { kind })?;
        if parsed.get_version_num() != 7 || parsed.get_variant() != Variant::RFC4122 {
            return Err(IdentityError { kind });
        }
    }
    Ok(value)
}

macro_rules! uuid_v7_id {
    ($($name:ident),+ $(,)?) => {$ (
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl TryFrom<String> for $name {
            type Error = IdentityError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                parse_uuid_v7(value, stringify!($name)).map(Self)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = IdentityError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::try_from(value.to_owned())
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self { value.0 }
        }

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IdentityError> {
                Self::try_from(value.into())
            }

            #[must_use]
            pub fn as_str(&self) -> &str { &self.0 }
        }
    )+};
}

uuid_v7_id!(
    BoardId,
    FrameId,
    ForwardId,
    InstallationId,
    ProjectId,
    RequestId,
    SessionId,
    TransferId
);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("epoch must be at least 1")]
pub struct EpochError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct Epoch(u64);

impl TryFrom<u64> for Epoch {
    type Error = EpochError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        (value > 0).then_some(Self(value)).ok_or(EpochError)
    }
}

impl Epoch {
    pub fn new(value: u64) -> Result<Self, EpochError> {
        Self::try_from(value)
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<Epoch> for u64 {
    fn from(value: Epoch) -> Self {
        value.0
    }
}

#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct CommitId(u64);

impl CommitId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<u64> for CommitId {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}
