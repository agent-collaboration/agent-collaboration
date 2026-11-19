//! Sender identity and labels resolved for the delivery frame.
use crate::{PersonId, ValueValidationError};
use collaboration_mail_protocol::SessionDisplayName;
use message_board::MachineLabel;
use serde::{Deserialize, Serialize};
use workspace_ids::SessionRef;

/// A validated label with the same text constraints as a session display name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PersonDisplayName(SessionDisplayName);

impl PersonDisplayName {
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<String> for PersonDisplayName {
    type Error = ValueValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        SessionDisplayName::try_from(value)
            .map(Self)
            .map_err(|_| ValueValidationError {
                field: "displayName",
                requirement: "must be 1 to 120 characters without controls or header arrows",
            })
    }
}

impl From<PersonDisplayName> for String {
    fn from(value: PersonDisplayName) -> Self {
        value.0.into()
    }
}

impl schemars::JsonSchema for PersonDisplayName {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PersonDisplayName".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        SessionDisplayName::json_schema(generator)
    }
}

/// An envelope-only sender resolved from the stored actor at push time.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ResolvedSender {
    Session {
        session: SessionRef,
        display_name: Option<SessionDisplayName>,
        machine_label: Option<MachineLabel>,
    },
    Person {
        person_id: PersonId,
        display_name: Option<PersonDisplayName>,
    },
    Central {},
}
