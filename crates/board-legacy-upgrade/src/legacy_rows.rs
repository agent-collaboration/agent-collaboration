//! Private source shapes retain the legacy schema's names; domain parsing is explicit.
use message_board::*;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Deserialize)]
pub(crate) struct LegacyWorkspace {
    pub project_id: WorkspaceId,
    #[serde(deserialize_with = "stored_name")]
    pub name: ResourceName,
    pub description: Description,
}
#[derive(Clone, Deserialize)]
pub(crate) struct LegacyBoard {
    pub board_id: BoardId,
    pub project_id: WorkspaceId,
    #[serde(deserialize_with = "stored_name")]
    pub name: ResourceName,
    pub description: Description,
    pub state: fleet_admission::ImportedBoardState,
}
#[derive(Clone, Deserialize)]
pub(crate) struct LegacyProject {
    pub topic_id: ProjectId,
    pub board_id: BoardId,
    #[serde(deserialize_with = "stored_name")]
    pub name: ResourceName,
    pub description: Description,
}
#[derive(Clone, Deserialize)]
pub(crate) struct LegacyIdentity {
    pub identity_key: String,
    pub kind: String,
    pub service_id: Option<ServiceId>,
    pub endpoint_id: Option<EndpointId>,
    pub session_id: Option<SessionId>,
    pub human_id: Option<HumanId>,
}
#[derive(Deserialize)]
pub(crate) struct LegacyThread {
    pub root_id: ThreadId,
    pub state: LegacyThreadState,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LegacyThreadState {
    Unresolved,
    Resolved,
}

fn stored_name<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<ResourceName, D::Error> {
    let text = String::deserialize(deserializer)?;
    if text.trim() != text {
        return Err(serde::de::Error::custom("stored name is not canonical"));
    }
    ResourceName::try_from(text).map_err(serde::de::Error::custom)
}
#[derive(Clone, Deserialize)]
pub(crate) struct LegacyPost {
    pub message_id: PostId,
    pub topic_id: ProjectId,
    pub board_id: BoardId,
    pub root_id: Option<ThreadId>,
    pub actor_key: String,
    pub acting_for_key: Option<String>,
    pub text: String,
    pub posted_from_activity: Option<u64>,
}
#[derive(Clone, Deserialize)]
pub(crate) struct LegacyActivity {
    pub activity_sequence: u64,
    pub topic_id: ProjectId,
    pub root_id: Option<ThreadId>,
    pub kind: ActivityKind,
    pub actor_key: String,
    pub message_id: Option<PostId>,
    pub participant_key: Option<String>,
    pub participant_role: Option<ParticipantRole>,
    pub replaced_participant_key: Option<String>,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ActivityKind {
    MainMessageCreated,
    ThreadMessageCreated,
    ParticipantJoined,
    ParticipantLeft,
    OrchestratorReplaced,
    ImplementerReplaced,
    ThreadResolved,
    ThreadUnresolved,
}
#[derive(Clone, Deserialize)]
pub(crate) struct LegacyParticipant {
    pub reader_key: String,
    pub root_id: ThreadId,
    pub role: ParticipantRole,
    pub note: Option<ParticipantNote>,
    pub joined_at_activity: u64,
    pub last_seen_activity: u64,
    pub closed_at_activity: Option<u64>,
    pub closed_reason: Option<ParticipantClosedReason>,
    pub replaced_by: Option<String>,
}
#[derive(Clone, Deserialize)]
pub(crate) struct LegacyBookmark {
    pub reader_key: String,
    pub scope_id: String,
    pub through_activity: u64,
}
#[derive(Clone, Deserialize)]
pub(crate) struct LegacyWatch {
    pub reader_key: String,
    pub scope_id: String,
    pub starts_after_activity: u64,
    pub active: bool,
}
#[derive(Clone, Deserialize)]
pub(crate) struct LegacyTracking {
    pub reader_key: String,
    pub project_id: WorkspaceId,
    pub main_start: Option<u64>,
}
#[derive(Clone, Deserialize)]
pub(crate) struct LegacySubscription {
    pub reader_key: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub mode: SubscriptionMode,
    pub when_idle: WhenIdle,
    pub quiet_seconds: u64,
    pub cap_seconds: u64,
    pub lifetime_seconds: u64,
    pub renewed_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub state: MembershipState,
    #[serde(deserialize_with = "legacy_end_reason")]
    pub end_reason: Option<EndReason>,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    pub generation: SubscriptionGeneration,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MembershipState {
    Active,
    Draining,
    Ended,
}

fn legacy_end_reason<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<EndReason>, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    value
        .map(|value| match value.as_str() {
            "left" => Ok(EndReason::Left),
            "replaced" => Ok(EndReason::Replaced),
            "resolved" => Ok(EndReason::Resolved),
            "cancelled" => Ok(EndReason::Cancelled),
            "expired" => Ok(EndReason::Expired),
            _ => Err(serde::de::Error::custom("unknown legacy membership ending")),
        })
        .transpose()
}
#[derive(Clone, Deserialize)]
pub(crate) struct LegacyRepository {
    pub project_id: WorkspaceId,
    pub kind: String,
    pub origin: Option<NormalizedOrigin>,
    pub service_id: Option<ServiceId>,
    pub common_directory: Option<CommonDirectory>,
}

pub(crate) struct LegacyStore {
    pub workspaces: Vec<LegacyWorkspace>,
    pub boards: Vec<LegacyBoard>,
    pub projects: Vec<LegacyProject>,
    pub actors: BTreeMap<String, Identity>,
    pub posts: BTreeMap<PostId, LegacyPost>,
    pub threads: BTreeMap<ThreadId, ThreadState>,
    pub activity: Vec<LegacyActivity>,
    pub participants: Vec<LegacyParticipant>,
    pub project_marks: Vec<LegacyBookmark>,
    pub thread_marks: Vec<LegacyBookmark>,
    pub project_watches: Vec<LegacyWatch>,
    pub thread_watches: Vec<LegacyWatch>,
    pub tracking: Vec<LegacyTracking>,
    pub subscriptions: Vec<LegacySubscription>,
    pub repositories: Vec<LegacyRepository>,
    pub references: BTreeMap<PostId, Vec<ReferenceTarget>>,
    pub last_recorded_at: Option<chrono::DateTime<chrono::Utc>>,
}
