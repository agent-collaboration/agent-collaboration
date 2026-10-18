//! Semantic operations carry their revision precondition at the operation that needs it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ActingForIdentity, AssignmentRevision, AuthorityRole, CloseReason, EpochNumber,
    EvidencePointer, Identity, ProjectId, ProtocolVersion, RequestId, ResourceName, RoleSubject,
    SubjectRevision, SubtaskId, SubtaskPlace, TaskAction, TaskId, TaskLink, TaskStatusChange,
    TaskTitle, TrailSubject, TrailTag, TrailText, WorkTrailRowId, WorkstreamId,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectRequest {
    pub request_id: RequestId,
    pub project_id: ProjectId,
    pub actor: Identity,
    pub acting_for: Option<ActingForIdentity>,
    pub observed_epoch: Option<EpochNumber>,
    pub resubmits: Option<RequestId>,
    pub protocol_version: ProtocolVersion,
    pub operation: BoardOperation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BoardOperation {
    WorkstreamCreate {
        workstream_id: WorkstreamId,
        name: ResourceName,
        goal: String,
    },
    WorkstreamRename {
        workstream_id: WorkstreamId,
        name: ResourceName,
        expected_revision: SubjectRevision,
    },
    WorkstreamClose {
        workstream_id: WorkstreamId,
        reason: CloseReason,
        note: String,
        expected_revision: SubjectRevision,
    },
    TaskCreate {
        task_id: TaskId,
        workstream_id: WorkstreamId,
        title: TaskTitle,
        body: String,
        blocked_by: Vec<TaskLinkTarget>,
        related_to: Vec<TaskLinkTarget>,
    },
    TaskUpdate {
        task_id: TaskId,
        title: TaskTitle,
        body: String,
        expected_revision: SubjectRevision,
    },
    TaskMoveWorkstream {
        task_id: TaskId,
        workstream_id: WorkstreamId,
        expected_revision: SubjectRevision,
    },
    TaskSetStatus {
        task_id: TaskId,
        status: TaskStatusChange,
        expected_revision: SubjectRevision,
    },
    TaskLink {
        link: TaskLinkInput,
    },
    TaskUnlink {
        link: TaskLink,
    },
    TaskAction {
        task_id: TaskId,
        action: TaskAction,
        expected_revision: SubjectRevision,
    },
    SubtaskAdd {
        subtask_id: SubtaskId,
        task_id: TaskId,
        text: TrailText,
        place: SubtaskPlace,
    },
    SubtaskCheck {
        subtask_id: SubtaskId,
        checked: bool,
    },
    SubtaskMove {
        subtask_id: SubtaskId,
        place: SubtaskPlace,
    },
    SubtaskRemove {
        subtask_id: SubtaskId,
    },
    WorkTrailAdd {
        row_id: WorkTrailRowId,
        subject: Option<TrailSubject>,
        text: TrailText,
        tags: Vec<TrailTag>,
        links: Vec<EvidencePointer>,
    },
    WorkTrailRedact {
        row_id: WorkTrailRowId,
        redacted: bool,
    },
    RoleGrant {
        subject: RoleSubject,
        role: AuthorityRole,
        holder: Identity,
        model: Option<ResourceName>,
        lineage: Option<ResourceName>,
        expected_revision: AssignmentRevision,
        owner_grant: Option<crate::OwnerGrantId>,
    },
    RoleRelease {
        subject: RoleSubject,
        role: AuthorityRole,
        holder: Option<crate::RecordedActor>,
        expected_revision: AssignmentRevision,
    },
    OwnerGrantRecord {
        grant_id: crate::OwnerGrantId,
        grant: crate::GrantKind,
    },
    RoleReplaceLead {
        new_holder: crate::RecordedActor,
        owner_grant: crate::OwnerGrantId,
        reason: String,
        expected_assignment_revision: AssignmentRevision,
    },
    RoleAcknowledgeCurrent {
        read_through: crate::FeedCursor,
    },
    ProjectPinDefinition {
        definition: crate::ProcessDefinitionRef,
        expected_revision: SubjectRevision,
    },
    ProjectPublishPlan {
        hash: crate::PlanRevisionHash,
        document: EvidencePointer,
        expected_revision: SubjectRevision,
    },
    ProjectClose {
        reason: String,
        expected_revision: SubjectRevision,
    },
    MoveRequest {
        move_id: crate::MoveId,
        move_name: crate::MoveName,
        evidence: Vec<crate::EvidenceRef>,
        reason: String,
        expected_revision: SubjectRevision,
        addressees: Vec<crate::NotifyTarget>,
    },
    FactRecord {
        fact: Box<crate::FactInput>,
    },
    ReadMarkInitialize {
        reader: Identity,
        scope: crate::ReadScope,
    },
    ReadMarkAdvance {
        reader: Identity,
        scope: crate::ReadScope,
        through: crate::FeedCursor,
    },
    ThreadCreate {
        post_id: crate::PostId,
        about: crate::ThreadSubject,
        text: crate::PostText,
        references: Vec<crate::RecordRef>,
        participation: Option<crate::ParticipantRole>,
        watch: bool,
        addressees: Vec<crate::NotifyTarget>,
    },
    ThreadRefile {
        thread_id: crate::ThreadId,
        about: crate::ThreadSubject,
    },
    ThreadPost {
        post_id: crate::PostId,
        thread_id: crate::ThreadId,
        text: crate::PostText,
        references: Vec<crate::RecordRef>,
        addressees: Vec<crate::NotifyTarget>,
    },
    ThreadResolve {
        thread_id: crate::ThreadId,
        reason: crate::ThreadResolutionReason,
    },
    ThreadUnresolve {
        thread_id: crate::ThreadId,
    },
    ThreadJoin {
        thread_id: crate::ThreadId,
        role: crate::ParticipantRole,
        watch: bool,
        mode: Option<crate::SubscriptionMode>,
        when_idle: Option<crate::WhenIdle>,
        replace: Option<Identity>,
        note: Option<crate::ParticipantNote>,
    },
    ThreadLeave {
        thread_id: crate::ThreadId,
        after_leave: Option<crate::LeaveFollowUp>,
    },
    ThreadWatch {
        thread_id: crate::ThreadId,
    },
    ThreadUnwatch {
        thread_id: crate::ThreadId,
    },
    ProjectWatch {},
    ProjectUnwatch {},
    ThreadSubscribe {
        scope: crate::SubscriptionScope,
        policy: crate::SubscriptionPolicyPatch,
    },
    ThreadUnsubscribe {
        scope: crate::SubscriptionScope,
    },
    ThreadRenew {
        scope: crate::SubscriptionScope,
    },
}

/// A write target carries its project so admission never needs cross-store discovery.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskLinkTarget {
    pub project: ProjectId,
    pub task: TaskId,
}
impl TaskLinkTarget {
    pub fn task_in_project(&self, project: &ProjectId) -> Result<TaskId, crate::TaskRefusal> {
        if &self.project != project {
            return Err(crate::TaskRefusal::DependencyCrossesProject(
                self.task.clone(),
            ));
        }
        Ok(self.task.clone())
    }
}
/// Request input differs from the stored link only in its qualified target.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskLinkInput {
    pub from: TaskId,
    pub kind: crate::TaskLinkKind,
    pub to: TaskLinkTarget,
}
impl TaskLinkInput {
    pub fn link_in_project(&self, project: &ProjectId) -> Result<TaskLink, crate::TaskRefusal> {
        Ok(TaskLink {
            from: self.from.clone(),
            kind: self.kind,
            to: self.to.task_in_project(project)?,
        })
    }
}
