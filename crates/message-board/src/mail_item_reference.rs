//! A project reference names a mail-owned item without creating another identity.
use crate::{IdentityValidationError, RecordId};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MailItemRef(RecordId);

impl TryFrom<String> for MailItemRef {
    type Error = IdentityValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        RecordId::try_from(value)
            .map(Self)
            .map_err(|error| IdentityValidationError {
                field: "MailItemRef",
                requirement: error.requirement,
            })
    }
}

impl From<MailItemRef> for String {
    fn from(value: MailItemRef) -> Self {
        value.0.into()
    }
}

impl MailItemRef {
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl JsonSchema for MailItemRef {
    fn schema_name() -> Cow<'static, str> {
        "ProjectMailItemRef".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        RecordId::json_schema(generator)
    }
}

#[cfg(test)]
#[path = "mail_item_reference_tests.rs"]
mod tests;
