//! The machine identifiers device login and presence carry. On the wire each is
//! a canonical lowercase UUIDv7 string, the same JSON the old fleet protocol
//! types produced, so the contracts and their schema fixtures are unchanged.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{kind} must be a canonical UUIDv7")]
pub struct MachineIdError {
    kind: &'static str,
}

macro_rules! machine_id {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(Uuid);

        impl $name {
            pub fn new(value: Uuid) -> Result<Self, MachineIdError> {
                if !runtime_primitives::uuid_v7(value) {
                    return Err(MachineIdError { kind: stringify!($name) });
                }
                Ok(Self(value))
            }
            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }
        impl TryFrom<Uuid> for $name {
            type Error = MachineIdError;
            fn try_from(value: Uuid) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
        impl From<$name> for Uuid {
            fn from(value: $name) -> Uuid {
                value.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = MachineIdError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                if !runtime_primitives::canonical_uuid_v7(&value) {
                    return Err(MachineIdError { kind: stringify!($name) });
                }
                let parsed = Uuid::parse_str(&value)
                    .map_err(|_| MachineIdError { kind: stringify!($name) })?;
                Self::new(parsed)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0.to_string()
            }
        }
        impl std::str::FromStr for $name {
            type Err = MachineIdError;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::try_from(value.to_owned())
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(formatter)
            }
        }
    )+};
}

machine_id!(PeerId, ConnectionId);
