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
    ($($(#[$attribute:meta])* $name:ident),+ $(,)?) => {$ (
        #[derive(
            Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
            schemars::JsonSchema,
        )]
        #[serde(try_from = "String", into = "String")]
        $(#[$attribute])*
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
    /// The Router install's durable identity, shared with central.
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"))]
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
    ReadMarkId,
    MessageId,
    NotificationWindowId,
    NotificationWindowItemId,
    WakeId,
    AutomationId
);

/// One agent conversation, as its provider (Claude Code, Codex) minted it:
/// stored as given, 1 to 4096 UTF-8 bytes without NUL.
#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(try_from = "String", into = "String")]
pub struct SessionId(String);
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("SessionId must contain 1 to 4096 UTF-8 bytes without NUL")]
pub struct SessionIdError;
impl TryFrom<String> for SessionId {
    type Error = SessionIdError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() || value.len() > 4096 || value.contains('\0') {
            return Err(SessionIdError);
        }
        Ok(Self(value))
    }
}
impl std::str::FromStr for SessionId {
    type Err = SessionIdError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value.to_owned())
    }
}
impl From<SessionId> for String {
    fn from(value: SessionId) -> Self {
        value.0
    }
}
impl std::fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl SessionId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The one session address (owner, 2026-11-18): the Router install that holds
/// the session, by its machine id, and the provider's session id. Used on the
/// wire, at central and by agents. It has no endpoint: which harness holds a
/// session is the Router's own business.
#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRef {
    pub machine_id: MachineId,
    pub session_id: SessionId,
}

#[cfg(test)]
#[path = "identifier_test.rs"]
mod tests;
