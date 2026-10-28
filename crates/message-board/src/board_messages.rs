use crate::{
    BoardId, HumanId, Identity, ImplementerHolder, OrchestratorHolder, ParticipantRole, PostId,
    PostText, ProjectId, ThreadId, ThreadState, WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ops::Deref;

pub const MAX_MESSAGE_REFERENCES: usize = 64;

#[derive(Debug, thiserror::Error, Clone, Eq, PartialEq)]
#[error("references must contain at most 64 distinct targets")]
pub struct InvalidPostReferences;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PostReferences(Vec<ReferenceTarget>);

#[derive(JsonSchema)]
#[schemars(transparent)]
#[expect(
    dead_code,
    reason = "The derive reads this field's schema; no schema-carrier value is constructed."
)]
struct PostReferencesSchema(#[schemars(length(max = 64))] Vec<ReferenceTarget>);

impl JsonSchema for PostReferences {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PostReferences".into()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        PostReferencesSchema::json_schema(generator)
    }
}

impl Serialize for PostReferences {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PostReferences {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let references = Vec::<ReferenceTarget>::deserialize(deserializer)?;
        Self::try_from(references).map_err(serde::de::Error::custom)
    }
}

impl TryFrom<Vec<ReferenceTarget>> for PostReferences {
    type Error = InvalidPostReferences;
    fn try_from(references: Vec<ReferenceTarget>) -> Result<Self, Self::Error> {
        if references.len() > MAX_MESSAGE_REFERENCES
            || references.iter().collect::<HashSet<_>>().len() != references.len()
        {
            return Err(InvalidPostReferences);
        }
        Ok(Self(references))
    }
}
impl From<PostReferences> for Vec<ReferenceTarget> {
    fn from(references: PostReferences) -> Self {
        references.0
    }
}
impl Deref for PostReferences {
    type Target = [ReferenceTarget];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl PostReferences {
    #[must_use]
    pub fn as_slice(&self) -> &[ReferenceTarget] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct ActivitySequence(u64);

#[derive(Debug, thiserror::Error, Clone, Eq, PartialEq)]
#[error("activity sequence must be between 0 and the maximum SQLite signed integer")]
pub struct InvalidActivitySequence;

impl TryFrom<u64> for ActivitySequence {
    type Error = InvalidActivitySequence;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        (value <= i64::MAX as u64)
            .then_some(Self(value))
            .ok_or(InvalidActivitySequence)
    }
}
impl From<ActivitySequence> for u64 {
    fn from(sequence: ActivitySequence) -> Self {
        sequence.0
    }
}
impl JsonSchema for ActivitySequence {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ActivitySequence".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"integer","minimum":0,"maximum":9223372036854775807_u64})
    }
}
impl ActivitySequence {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ActingForIdentity {
    #[serde(rename_all = "camelCase")]
    Human { human_id: HumanId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Placement {
    #[serde(rename_all = "camelCase")]
    Project { project_id: ProjectId },
    #[serde(rename_all = "camelCase")]
    Thread { thread_id: ThreadId },
}

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ReferenceTarget {
    #[serde(rename_all = "camelCase")]
    Post { post_id: PostId },
    #[serde(rename_all = "camelCase")]
    Thread { thread_id: ThreadId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ReadScope {
    #[serde(rename_all = "camelCase")]
    Project { project_id: ProjectId },
    #[serde(rename_all = "camelCase")]
    Thread { thread_id: ThreadId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum InboxScope {
    #[serde(rename_all = "camelCase")]
    Workspace { workspace_id: WorkspaceId },
    #[serde(rename_all = "camelCase")]
    Board { board_id: BoardId },
    #[serde(rename_all = "camelCase")]
    Project { project_id: ProjectId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum PostListScope {
    #[serde(rename_all = "camelCase")]
    Project {
        project_id: ProjectId,
    },
    #[serde(rename_all = "camelCase")]
    Thread {
        thread: crate::ThreadRef,
    },
    #[serde(rename_all = "camelCase")]
    Board {
        board_id: BoardId,
    },
    #[serde(rename_all = "camelCase")]
    Workspace {
        workspace_id: WorkspaceId,
    },
    AllWorkspaces,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum PostSelection {
    Latest,
    #[serde(rename_all = "camelCase")]
    AfterPosition {
        after: crate::ReadPosition,
    },
    #[serde(rename_all = "camelCase")]
    Range {
        from: crate::ReadPosition,
        to: crate::ReadPosition,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum PostSelectionWire {
    Latest {},
    #[serde(rename_all = "camelCase")]
    AfterPosition {
        after: crate::ReadPosition,
    },
    #[serde(rename_all = "camelCase")]
    Range {
        from: crate::ReadPosition,
        to: crate::ReadPosition,
    },
}

impl<'de> Deserialize<'de> for PostSelection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let selection = match PostSelectionWire::deserialize(deserializer)? {
            PostSelectionWire::Latest {} => Self::Latest,
            PostSelectionWire::AfterPosition { after } => Self::AfterPosition { after },
            PostSelectionWire::Range { from, to } => Self::Range { from, to },
        };
        selection.validate().map_err(serde::de::Error::custom)?;
        Ok(selection)
    }
}

impl PostSelection {
    pub fn validate(&self) -> Result<(), InvalidPostSelection> {
        match self {
            Self::Range { from, to } if !from.precedes_or_equals(to) => Err(InvalidPostSelection),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, thiserror::Error, Clone, Eq, PartialEq)]
#[error("range start must not exceed range end")]
pub struct InvalidPostSelection;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PostOrdering {
    NewestFirst,
    OldestFirst,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Post {
    pub envelope: crate::RecordEnvelope,
    pub post_id: PostId,
    pub board_id: BoardId,
    pub project_id: ProjectId,
    pub placement: Placement,
    pub actor: Identity,
    pub acting_for: Option<ActingForIdentity>,
    pub text: PostText,
    pub references: PostReferences,
    pub position: crate::RecordPlacement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posted_as_role: Option<ParticipantRole>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Thread {
    pub envelope: crate::RecordEnvelope,
    pub thread_id: ThreadId,
    pub about: crate::ThreadSubject,
    pub imported: bool,
    pub state: ThreadState,
    pub orchestrator: Option<OrchestratorHolder>,
    pub implementer: Option<ImplementerHolder>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryRange {
    pub scope: PostListScope,
    pub selection: PostSelection,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatchStatus {
    pub watching: bool,
    pub starts_after: Option<crate::FeedCursor>,
    pub earlier_unwatched_range: Option<HistoryRange>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum InboxActivity {
    #[serde(rename_all = "camelCase")]
    PostCreated {
        workspace_id: WorkspaceId,
        position: crate::RecordPlacement,
        acknowledgement_scope: ReadScope,
        post: Box<Post>,
    },
    #[serde(rename_all = "camelCase")]
    ThreadStateChanged {
        workspace_id: WorkspaceId,
        board_id: BoardId,
        position: crate::RecordPlacement,
        acknowledgement_scope: ReadScope,
        thread_id: ThreadId,
        project_id: ProjectId,
        actor: Identity,
        state: ThreadState,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceUnreadSummary {
    pub workspace_id: WorkspaceId,
    pub has_unread: bool,
    pub main_tracking_initialized: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bookmark {
    pub reader: Identity,
    pub scope: ReadScope,
    pub through: crate::FeedCursor,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum InboxInitializationStatus {
    NotApplicable,
    Existing,
    #[serde(rename_all = "camelCase")]
    Initialized {
        starts_after: crate::FeedCursor,
        earlier_history_range: Option<Box<HistoryRange>>,
        message: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostPage {
    pub scope: PostListScope,
    pub selection: PostSelection,
    pub ordering: PostOrdering,
    pub records: Vec<Post>,
    pub next: crate::AggregateCursor,
    pub high_water: std::collections::BTreeMap<ProjectId, crate::FeedCursor>,
    pub unavailable: Vec<ProjectId>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InboxPage {
    pub workspace_id: WorkspaceId,
    pub scope: InboxScope,
    pub reader: Identity,
    pub read_mode: InboxReadMode,
    pub ordering: PostOrdering,
    pub high_water: std::collections::BTreeMap<ProjectId, crate::FeedCursor>,
    pub next: crate::AggregateCursor,
    pub unavailable: Vec<ProjectId>,
    pub records: Vec<InboxActivity>,
    pub next_cursor: Option<String>,
    pub initializations: Vec<crate::ReadInitializationOutcome>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum InboxReadMode {
    #[default]
    Unread,
    Latest,
}
