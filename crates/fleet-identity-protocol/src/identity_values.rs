//! Validated identifiers and launch values shared across runtime boundaries.
use message_board::{IdentityValidationError, RecordId, ServiceId};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

macro_rules! identity_value {
    ($name:ident, $inner:ty) => {
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name($inner);
        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(generator: &mut SchemaGenerator) -> Schema {
                <$inner as JsonSchema>::json_schema(generator)
            }
        }
        impl TryFrom<String> for $name {
            type Error = IdentityValidationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                <$inner>::try_from(value).map(Self)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0.into()
            }
        }
        impl $name {
            #[must_use]
            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }
    };
}
identity_value!(OrganizationId, RecordId);
identity_value!(EnrollmentId, RecordId);
identity_value!(RemoteStartId, RecordId);
identity_value!(IsolationInstanceId, RecordId);
identity_value!(InstallationId, ServiceId);

macro_rules! bounded_value {
    ($name:ident, $max:expr, $pattern:expr, $validate:expr) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl TryFrom<String> for $name {
            type Error = IdentityValidationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                if value.is_empty() || value.len() > $max || !($validate)(&value) {
                    return Err(IdentityValidationError { field: stringify!($name), requirement: "is not a valid bounded value" });
                }
                Ok(Self(value))
            }
        }
        impl From<$name> for String { fn from(value: $name) -> Self { value.0 } }
        impl $name { #[must_use] pub fn as_str(&self) -> &str { &self.0 } }
        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> { stringify!($name).into() }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({"type":"string","minLength":1,"maxLength":$max,"pattern":$pattern,"x-maxUtf8Bytes":$max})
            }
        }
    };
}
// The wire uses Unix paths on every host; std::path::is_absolute is false on wasm32-unknown-unknown.
bounded_value!(AbsolutePath, 4096, "^/[^\\u0000]*$", |value: &str| !value
    .contains('\0')
    && value.starts_with('/'));
bounded_value!(RepositoryLocator, 2048, "^[^\\u0000]+$", |value: &str| {
    !value.contains('\0')
});
bounded_value!(ModelName, 256, "^[^\\u0000]+$", |value: &str| !value
    .contains('\0'));
bounded_value!(
    CommitId,
    64,
    "^(?:[0-9a-f]{40}|[0-9a-f]{64})$",
    |value: &str| matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixSeconds(pub i64);
impl JsonSchema for UnixSeconds {
    fn schema_name() -> Cow<'static, str> {
        "UnixSeconds".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        i64::json_schema(generator)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixMillis(pub i64);
impl JsonSchema for UnixMillis {
    fn schema_name() -> Cow<'static, str> {
        "UnixMillis".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        i64::json_schema(generator)
    }
}

/// A report or attach counter that every JSON consumer can read exactly.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct SequenceNumber(u64);

impl TryFrom<u64> for SequenceNumber {
    type Error = IdentityValidationError;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value <= 9_007_199_254_740_991 {
            Ok(Self(value))
        } else {
            Err(IdentityValidationError {
                field: "sequence",
                requirement: "must remain below 2^53 for exact JSON transport",
            })
        }
    }
}
impl From<SequenceNumber> for u64 {
    fn from(value: SequenceNumber) -> Self {
        value.0
    }
}
impl SequenceNumber {
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}
impl JsonSchema for SequenceNumber {
    fn schema_name() -> Cow<'static, str> {
        "SequenceNumber".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"integer","minimum":0,"maximum":9007199254740991_u64})
    }
}
