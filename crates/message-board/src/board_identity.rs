use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use uuid::{Uuid, Variant};

/// Canonical UUID syntax accepts every version and variant already supported by ServiceId.
pub const CANONICAL_UUID_PATTERN: &str =
    "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$(?![\\s\\S])";
/// Endpoint syntax shared with the Router's public identity schema.
pub const ENDPOINT_ID_PATTERN: &str = "^[a-z][a-z0-9-]{0,63}$(?![\\s\\S])";
/// Opaque session and human identities permit every character except NUL.
pub const OPAQUE_ID_PATTERN: &str = "^[^\\u0000]+$";

#[derive(Debug, thiserror::Error, Clone, Eq, PartialEq)]
#[error("{field} {requirement}")]
pub struct IdentityValidationError {
    pub field: &'static str,
    pub requirement: &'static str,
}

macro_rules! uuid_v7_identity {
    ($($name:ident),+ $(,)?) => {$ (
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> { stringify!($name).into() }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({"type":"string","minLength":36,"maxLength":36,
                    "pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"})
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdentityValidationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                let parsed = Uuid::parse_str(&value).map_err(|_| IdentityValidationError {
                    field: stringify!($name), requirement: "must be a canonical lowercase RFC UUIDv7",
                })?;
                if parsed.get_version_num() != 7
                    || parsed.get_variant() != Variant::RFC4122
                    || parsed.hyphenated().to_string() != value
                {
                    return Err(IdentityValidationError {
                        field: stringify!($name), requirement: "must be a canonical lowercase RFC UUIDv7",
                    });
                }
                Ok(Self(value))
            }
        }

        impl From<$name> for String { fn from(value: $name) -> Self { value.0 } }
        impl $name {
            #[cfg(not(target_arch = "wasm32"))]
            #[must_use] pub fn generate() -> Self { Self(Uuid::now_v7().hyphenated().to_string()) }
            #[must_use] pub fn as_str(&self) -> &str { &self.0 }
        }
    )+};
}

uuid_v7_identity!(ProjectId, BoardId);

uuid_v7_identity!(
    WorkspaceId,
    WorkstreamId,
    TaskId,
    TaskLinkEventId,
    SubtaskId,
    AdrId,
    ThreadId,
    PostId,
    RoleAssignmentId,
    OwnerGrantId,
    MoveId,
    FactId,
    NotifyIntentId,
    RequestId,
    WorkTrailRowId,
    TrailRedactionId,
    RecordId,
    EpochLineId
);

uuid_v7_identity!(TransferId);

impl From<PostId> for ThreadId {
    fn from(root_post: PostId) -> Self {
        Self(root_post.0)
    }
}
impl From<ThreadId> for PostId {
    fn from(thread: ThreadId) -> Self {
        Self(thread.0)
    }
}

macro_rules! record_identity {
    ($($identity:ident => $kind:ident),+ $(,)?) => {$ (
        impl From<$identity> for RecordId {
            fn from(identity: $identity) -> Self { Self(identity.0) }
        }
        impl From<$identity> for crate::RecordRef {
            fn from(identity: $identity) -> Self {
                Self { kind: crate::RecordKind::$kind, id: identity.into() }
            }
        }
    )+};
}
record_identity!(WorkspaceId => Workspace, BoardId => Board, ProjectId => Project,
    WorkstreamId => Workstream, TaskId => Task, TaskLinkEventId => TaskLinkEvent, SubtaskId => Subtask, AdrId => Adr,
    ThreadId => Thread, PostId => Post, RoleAssignmentId => RoleAssignment,
    FactId => Fact, NotifyIntentId => NotifyIntent, RequestId => Request,
    WorkTrailRowId => WorkTrailRow, TrailRedactionId => TrailRedaction,
    OwnerGrantId => OwnerGrant);

macro_rules! validated_identity_string {
    ($name:ident, $field:literal, $min:expr, $max:expr, $pattern:expr, $validator:expr, $requirement:literal) => {
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> { stringify!($name).into() }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({"type":"string","minLength":$min,"maxLength":$max,"pattern":$pattern,"x-maxUtf8Bytes":$max})
            }
        }
        impl TryFrom<String> for $name {
            type Error = IdentityValidationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                if !($min..=$max).contains(&value.len()) || !($validator)(&value) {
                    return Err(IdentityValidationError {
                        field: $field,
                        requirement: $requirement,
                    });
                }
                Ok(Self(value))
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl $name {
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

validated_identity_string!(
    ServiceId,
    "serviceId",
    36,
    36,
    CANONICAL_UUID_PATTERN,
    |value: &str| {
        Uuid::parse_str(value).is_ok_and(|parsed| parsed.hyphenated().to_string() == value)
    },
    "must be a canonical lowercase RFC UUID"
);
validated_identity_string!(
    EndpointId,
    "endpointId",
    1,
    64,
    ENDPOINT_ID_PATTERN,
    |value: &str| {
        value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    },
    "must start with a lowercase letter and contain only lowercase letters, digits, or hyphens"
);
validated_identity_string!(
    SessionId,
    "sessionId",
    1,
    4096,
    OPAQUE_ID_PATTERN,
    |value: &str| !value.contains('\0'),
    "must contain 1 to 4096 UTF-8 bytes without NUL"
);
validated_identity_string!(
    HumanId,
    "humanId",
    1,
    4096,
    OPAQUE_ID_PATTERN,
    |value: &str| !value.contains('\0'),
    "must contain 1 to 4096 UTF-8 bytes without NUL"
);

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionEndpointRef {
    pub service_id: ServiceId,
    pub endpoint_id: EndpointId,
}

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRef {
    pub endpoint: SessionEndpointRef,
    pub session_id: SessionId,
}

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Identity {
    #[serde(rename_all = "camelCase")]
    Session { session: SessionRef },
    #[serde(rename_all = "camelCase")]
    Human { human_id: HumanId },
}

impl Identity {
    #[must_use]
    pub fn as_human(&self) -> Option<&HumanId> {
        match self {
            Self::Human { human_id } => Some(human_id),
            Self::Session { .. } => None,
        }
    }

    #[must_use]
    pub fn short_form(&self) -> String {
        match self {
            Self::Session { session } => {
                let endpoint = session.endpoint.endpoint_id.as_str();
                let harness = endpoint
                    .split_once('-')
                    .map_or(endpoint, |(harness, _attachment)| harness);
                format!("{harness}:{}", session.session_id.as_str())
            }
            Self::Human { human_id } => human_id.as_str().to_owned(),
        }
    }
}
