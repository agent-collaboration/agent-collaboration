//! Stable record references and evidence pointers carry meaning across stores.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

use crate::{NormalizedOrigin, RecordId, TextValidationError};

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum RecordKind {
    Workspace,
    Board,
    Adr,
    Project,
    Workstream,
    Task,
    TaskLinkEvent,
    Subtask,
    Thread,
    ThreadRefiled,
    Post,
    RoleAssignment,
    OwnerGrant,
    StandDownAcknowledgement,
    Fact,
    PlanRevision,
    ReadMark,
    NotifyIntent,
    SubscriptionMembership,
    Request,
    WorkTrailRow,
    TrailRedaction,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordRef {
    pub kind: RecordKind,
    pub id: RecordId,
}

macro_rules! digest_value {
    ($name:ident, $field:literal, $valid_length:expr, $minimum:literal, $maximum:literal) => {
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl TryFrom<String> for $name {
            type Error = TextValidationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                if !($valid_length)(value.len()) || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
                    return Err(TextValidationError { field: $field, requirement: "must be a complete lowercase hexadecimal digest" });
                }
                Ok(Self(value))
            }
        }
        impl From<$name> for String { fn from(value: $name) -> Self { value.0 } }
        impl $name { #[must_use] pub fn as_str(&self) -> &str { &self.0 } }
        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> { stringify!($name).into() }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                schemars::json_schema!({"type":"string","minLength":$minimum,"maxLength":$maximum,"pattern":"^[0-9a-f]+$"})
            }
        }
    };
}

digest_value!(
    CanonicalHash,
    "canonicalHash",
    |length: usize| length == 64,
    64,
    64
);
impl CanonicalHash {
    pub(crate) fn from_sha256(bytes: [u8; 32]) -> Self {
        Self(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}
digest_value!(
    SchemaFingerprint,
    "schemaFingerprint",
    |length: usize| length == 64,
    64,
    64
);
digest_value!(
    PlanRevisionHash,
    "planRevision",
    |length: usize| length == 64,
    64,
    64
);
digest_value!(
    CommitSha,
    "sha",
    |length: usize| matches!(length, 40 | 64),
    40,
    64
);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EvidenceUrl(String);

impl TryFrom<String> for EvidenceUrl {
    type Error = TextValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let parsed = url::Url::parse(&value).map_err(|_| TextValidationError {
            field: "url",
            requirement: "must be an absolute URL",
        })?;
        Ok(Self(parsed.to_string()))
    }
}
impl From<EvidenceUrl> for String {
    fn from(value: EvidenceUrl) -> Self {
        value.0
    }
}
impl EvidenceUrl {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl JsonSchema for EvidenceUrl {
    fn schema_name() -> Cow<'static, str> {
        "EvidenceUrl".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({"type":"string","format":"uri"})
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EvidencePointer {
    Commit {
        repository: NormalizedOrigin,
        sha: CommitSha,
    },
    PullRequest {
        repository: NormalizedOrigin,
        number: u64,
    },
    RepoPath {
        repository: NormalizedOrigin,
        path: String,
        commit: Option<CommitSha>,
    },
    Url {
        url: EvidenceUrl,
    },
    Record {
        record: RecordRef,
    },
}
