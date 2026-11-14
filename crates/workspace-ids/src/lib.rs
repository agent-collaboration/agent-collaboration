//! Workspace identifiers validate identity only; central owns minting and order.
use serde::{Deserialize, Serialize};
use uuid::{Uuid, Variant};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{field} must be a canonical lowercase RFC UUIDv7")]
pub struct IdValidationError {
    pub field: &'static str,
}

// Ported from message-board::board_identity, without the system-clock mint API.
macro_rules! workspace_identifiers {
    ($($name:ident),+ $(,)?) => {$ (
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl TryFrom<String> for $name {
            type Error = IdValidationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                let invalid = || IdValidationError { field: stringify!($name) };
                let parsed = Uuid::parse_str(&value).map_err(|_| invalid())?;
                if parsed.get_version_num() != 7 || parsed.get_variant() != Variant::RFC4122
                    || parsed.hyphenated().to_string() != value {
                    return Err(invalid());
                }
                Ok(Self(value))
            }
        }
        impl std::str::FromStr for $name {
            type Err = IdValidationError;
            fn from_str(value: &str) -> Result<Self, Self::Err> { Self::try_from(value.to_owned()) }
        }
        impl From<$name> for String { fn from(value: $name) -> Self { value.0 } }
        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
        impl $name {
            #[must_use] pub fn as_str(&self) -> &str { &self.0 }
        }
    )+};
}

workspace_identifiers!(
    WorkspaceId,
    RequestId,
    MachineId,
    PersonId,
    AreaId,
    ProjectId,
    InitiativeId,
    ActorIdentityId,
    ProjectRoleId,
    DeliverableId,
    DeliverableRoleId,
    DependencyId,
    IssueId,
    ThreadId,
    ThreadMemberId,
    PostId,
    WorkLogEntryId,
    WorkLogTagId,
    BacklogItemId,
    ReferenceId,
    LinkId,
    ChangeId,
    ReadMarkId
);

#[cfg(test)]
#[path = "identifier_test.rs"]
mod tests;
