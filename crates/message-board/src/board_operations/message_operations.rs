use crate::{
    ActingForIdentity, Identity, ImplementerHolder, OrchestratorHolder, Page, PageRequest,
    Participant, ParticipantNote, ParticipantRole, Post, PostId, PostListScope, PostPage,
    PostReferences, PostSelection, PostTarget, PostText, ProjectId, SubscriptionMode, Thread,
    ThreadReadRecord, ThreadRef, WatchStatus, WhenIdle, WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! contract {
    ($name:ident { $($(#[$meta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name { $($(#[$meta])* pub $field: $ty),* }
    };
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadPostRequest {
    pub request_id: crate::RequestId,
    pub protocol_version: crate::ProtocolVersion,
    pub resubmits: Option<crate::RequestId>,
    pub observed_epoch: Option<crate::EpochNumber>,
    pub post_id: PostId,
    pub placement: PostTarget,
    pub actor: Identity,
    pub acting_for: Option<ActingForIdentity>,
    pub text: PostText,
    pub references: PostReferences,
    pub addressees: Vec<crate::NotifyTarget>,
}

contract!(PostShowRequest { post_id: PostId });
/// A single read returns the message itself as the record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct PostShowResult(pub Post);
contract!(PostListRequest {
    scope: PostListScope,
    selection: PostSelection,
    page: PageRequest
});
contract!(PostListResult { page: PostPage });

contract!(ThreadShowRequest {
    thread: ThreadRef,
    reader: crate::ReadActor,
    #[serde(default)]
    include_redacted: bool
});
/// A single read returns the thread itself as the record, carrying the reader's
/// watch status alongside the thread's own fields.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThreadShowResult {
    pub project_id: ProjectId,
    pub read_mark: Option<crate::ReadMarkOutcome>,
    #[serde(flatten)]
    pub thread: Thread,
    pub subject: crate::ThreadSubjectView,
    pub watch_status: Option<WatchStatus>,
}
contract!(ThreadRefileRequest {
    request_id: crate::RequestId, protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>, observed_epoch: Option<crate::EpochNumber>,
    thread: ThreadRef, actor: Identity, acting_for: Option<ActingForIdentity>, about: crate::ThreadSubject
});
contract!(ThreadResolveRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    observed_epoch: Option<crate::EpochNumber>,
    thread: ThreadRef,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
    reason: crate::ThreadResolutionReason,
});

contract!(ThreadUnresolveRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    observed_epoch: Option<crate::EpochNumber>,
    thread: ThreadRef,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
});

contract!(ThreadWatchRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    observed_epoch: Option<crate::EpochNumber>,
    thread: ThreadRef,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
});

contract!(ThreadUnwatchRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    observed_epoch: Option<crate::EpochNumber>,
    thread: ThreadRef,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
});

contract!(ProjectWatchRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    observed_epoch: Option<crate::EpochNumber>,
    project_id: ProjectId,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
});

contract!(ThreadListRequest {
    workspace_id: WorkspaceId,
    reader: Identity,
    #[serde(default)]
    watched_only: bool,
    page: PageRequest,
});
contract!(ThreadListResult { page: Page<ThreadReadRecord> });

contract!(ThreadCreateRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    observed_epoch: Option<crate::EpochNumber>,
    post_id: PostId,
    project_id: ProjectId,
    about: crate::ThreadSubject,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
    text: PostText,
    references: PostReferences,
    addressees: Vec<crate::NotifyTarget>,
    role: Option<ParticipantRole>,
    watch: bool,
});

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ThreadCreatorParticipation {
    #[serde(rename_all = "camelCase")]
    Joined {
        participant: Box<Participant>,
    },
    NotJoined,
}

contract!(ThreadJoinRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    observed_epoch: Option<crate::EpochNumber>,
    thread: ThreadRef,
    actor: Identity,
    role: ParticipantRole,
    watch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")] mode: Option<SubscriptionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")] when_idle: Option<WhenIdle>,
    replace: Option<Identity>,
    note: Option<ParticipantNote>,
});

contract!(ThreadLeaveRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    observed_epoch: Option<crate::EpochNumber>,
    thread: ThreadRef,
    actor: Identity,
    after_leave: Option<crate::LeaveFollowUp>,
});

contract!(ThreadParticipantListRequest {
    thread: ThreadRef,
    page: PageRequest
});
contract!(ThreadParticipantListResult {
    project_id: ProjectId,
    page: Page<Participant>,
    orchestrator: Option<OrchestratorHolder>,
    implementer: Option<ImplementerHolder>
});

contract!(ThreadRenewRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    observed_epoch: Option<crate::EpochNumber>,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
    scope: crate::SubscriptionTarget,
});
