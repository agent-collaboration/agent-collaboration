//! Central-owned organization, issuer-decision and lookup contracts.
mod enrollment_records;
mod identity_decisions;
mod identity_lookups;
mod organization_commands;

pub use enrollment_records::*;
pub use identity_decisions::*;
pub use identity_lookups::*;
pub use organization_commands::*;

use message_board::{IdentityValidationError, RecordId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! central_identity {
    ($($name:ident),+ $(,)?) => {$ (
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(RecordId);
        impl JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> { stringify!($name).into() }
            fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                RecordId::json_schema(generator)
            }
        }
        impl TryFrom<String> for $name {
            type Error = IdentityValidationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                RecordId::try_from(value).map(Self)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self { value.0.into() }
        }
        impl $name { #[must_use] pub fn as_str(&self) -> &str { self.0.as_str() } }
    )+};
}
central_identity!(
    PersonId,
    ClientId,
    DeviceLoginRequestId,
    AdmissionId,
    CommandId
);
