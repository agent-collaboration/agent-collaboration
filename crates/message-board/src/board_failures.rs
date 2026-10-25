use crate::{
    BoardId, FeedCursor, Identity, ParticipantRole, PostId, PostText, ProjectId, ThreadId,
    WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BoardFailureKind {
    RoleRequired,
    ArchivedBoard,
    #[serde(rename = "postCooldown")]
    TopLevelPostCooldown,
    ReferenceTargetNotFound,
    InvalidIdentity,
    InvalidProjectName,
    InvalidRootPost,
    PositionBeyondLatest,
    InvalidAcknowledgement,
    InvalidCursor,
    InvalidField,
    InvalidShape,
    NameConflict,
    ResourceNotFound,
    ResourceAlreadyExists,
    OutcomeUnknown,
    BoardUnavailable,
    Overloaded,
    ThreadResolved,
    InvalidRecord,
    ParticipantRequired,
    OrchestratorRequired,
    OrchestratorAlreadyExists,
    ImplementerAlreadyExists,
    OrchestratorHandoverRequired,
    HandoverTargetNotParticipant,
    StaleOrchestrator,
    StaleImplementer,
    SelfReplace,
    OrchestratorRoleChange,
    SessionProjectPost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BoardFailureStage {
    Validation,
    Admission,
    Storage,
    Inspection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BoardNextAction {
    CorrectRequest,
    InspectResource,
    SelectDifferentName,
    RetryLater,
    UnresolveThread,
    PostThreadPost,
    CreateThread,
    JoinThread,
    ReplaceOrchestrator,
    ReplaceImplementer,
    LeaveWithHandoverOrResolve,
    JoinHandoverTarget,
    InspectParticipants,
    RepeatJoinWithoutReplace,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ResourceIdentity {
    #[serde(rename_all = "camelCase")]
    Workspace { workspace_id: WorkspaceId },
    #[serde(rename_all = "camelCase")]
    Board { board_id: BoardId },
    #[serde(rename_all = "camelCase")]
    Project { project_id: ProjectId },
    #[serde(rename_all = "camelCase")]
    Post { post_id: PostId },
    #[serde(rename_all = "camelCase")]
    Thread { thread_id: ThreadId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum BoardErrorDetails {
    #[serde(rename_all = "camelCase")]
    RoleRequired {
        project: ProjectId,
        actor: Box<Identity>,
        requirement: crate::RoleRequirement,
    },
    None,
    #[serde(rename_all = "camelCase")]
    FieldConstraint {
        field: String,
        requirement: String,
    },
    #[serde(rename_all = "camelCase")]
    Resource {
        resource: ResourceIdentity,
    },
    #[serde(rename_all = "camelCase")]
    Cooldown {
        retry_after_seconds: u64,
    },
    #[serde(rename_all = "camelCase")]
    ParticipantRefusal {
        #[serde(flatten)]
        refusal: Box<ParticipantRefusalDetails>,
    },
    #[serde(rename_all = "camelCase")]
    ThreadCreateRefusal {
        #[serde(flatten)]
        refusal: Box<ThreadCreateRefusalDetails>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ParticipantRefusalDetails {
    pub thread_id: ThreadId,
    pub missing_thread_ids: Vec<ThreadId>,
    pub actor: Identity,
    pub allowed_roles: Vec<ParticipantRole>,
    pub holder: Option<Identity>,
    pub holder_last_seen_position: Option<FeedCursor>,
    pub target: Option<Identity>,
    pub named_holder: Option<Identity>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadCreateRefusalDetails {
    pub project_id: ProjectId,
    pub actor: Identity,
    pub text: PostText,
    pub allowed_roles: Vec<ParticipantRole>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParticipantRefusal {
    pub kind: BoardFailureKind,
    pub message: String,
    pub next_action: BoardNextAction,
    pub thread_id: ThreadId,
    pub actor: Identity,
    pub holder: Option<Identity>,
    pub holder_last_seen_position: Option<FeedCursor>,
    pub target: Option<Identity>,
    pub named_holder: Option<Identity>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoardError {
    pub kind: BoardFailureKind,
    pub stage: BoardFailureStage,
    pub message: String,
    pub next_action: BoardNextAction,
    pub details: BoardErrorDetails,
}

impl BoardError {
    fn all_roles() -> Vec<ParticipantRole> {
        vec![
            ParticipantRole::Orchestrator,
            ParticipantRole::Implementer,
            ParticipantRole::Advisor,
            ParticipantRole::Reviewer,
            ParticipantRole::Participant,
        ]
    }

    #[must_use]
    pub fn participant_required(thread_id: ThreadId, actor: Identity) -> Self {
        Self::participants_required(thread_id.clone(), vec![thread_id], actor)
    }

    #[must_use]
    pub fn participants_required(
        thread_id: ThreadId,
        missing_thread_ids: Vec<ThreadId>,
        actor: Identity,
    ) -> Self {
        Self {
            kind: BoardFailureKind::ParticipantRequired,
            stage: BoardFailureStage::Admission,
            message: format!(
                "This session must join the Thread before session delivery. Run: agent-collaboration thread join --thread-id {} --actor self --role participant --no-watch --json",
                String::from(thread_id.clone())
            ),
            next_action: BoardNextAction::JoinThread,
            details: BoardErrorDetails::ParticipantRefusal {
                refusal: Box::new(ParticipantRefusalDetails {
                    thread_id,
                    missing_thread_ids,
                    actor,
                    allowed_roles: Self::all_roles(),
                    holder: None,
                    holder_last_seen_position: None,
                    target: None,
                    named_holder: None,
                }),
            },
        }
    }

    #[must_use]
    pub fn session_project_post(project_id: ProjectId, actor: Identity, text: PostText) -> Self {
        Self {
            kind: BoardFailureKind::SessionProjectPost,
            stage: BoardFailureStage::Admission,
            message: "A session creates a Thread with thread create and an explicit Role and Watch choice.".to_owned(),
            next_action: BoardNextAction::CreateThread,
            details: BoardErrorDetails::ThreadCreateRefusal {
                refusal: Box::new(ThreadCreateRefusalDetails {
                    project_id,
                    actor,
                    text,
                    allowed_roles: Self::all_roles(),
                }),
            },
        }
    }

    #[must_use]
    pub fn orchestrator_already_exists(
        thread_id: ThreadId,
        actor: Identity,
        holder: Identity,
        holder_last_seen_position: FeedCursor,
    ) -> Self {
        Self {
            kind: BoardFailureKind::OrchestratorAlreadyExists,
            stage: BoardFailureStage::Admission,
            message: "This Thread already has an Orchestrator. Name that holder explicitly to Replace it.".to_owned(),
            next_action: BoardNextAction::ReplaceOrchestrator,
            details: BoardErrorDetails::ParticipantRefusal {
                refusal: Box::new(ParticipantRefusalDetails {
                thread_id,
                missing_thread_ids: Vec::new(),
                actor,
                allowed_roles: vec![ParticipantRole::Orchestrator],
                holder: Some(holder),
                holder_last_seen_position: Some(holder_last_seen_position),
                target: None,
                named_holder: None,
                }),
            },
        }
    }

    #[must_use]
    pub fn implementer_already_exists(
        thread_id: ThreadId,
        actor: Identity,
        holder: Identity,
        holder_last_seen_position: FeedCursor,
    ) -> Self {
        Self {
            kind: BoardFailureKind::ImplementerAlreadyExists,
            stage: BoardFailureStage::Admission,
            message:
                "This Thread already has an Implementer. Name that holder explicitly to Replace it."
                    .to_owned(),
            next_action: BoardNextAction::ReplaceImplementer,
            details: BoardErrorDetails::ParticipantRefusal {
                refusal: Box::new(ParticipantRefusalDetails {
                    thread_id,
                    missing_thread_ids: Vec::new(),
                    actor,
                    allowed_roles: vec![ParticipantRole::Implementer],
                    holder: Some(holder),
                    holder_last_seen_position: Some(holder_last_seen_position),
                    target: None,
                    named_holder: None,
                }),
            },
        }
    }

    #[must_use]
    pub fn participant_refusal(refusal: ParticipantRefusal) -> Self {
        Self {
            kind: refusal.kind,
            stage: BoardFailureStage::Admission,
            message: refusal.message,
            next_action: refusal.next_action,
            details: BoardErrorDetails::ParticipantRefusal {
                refusal: Box::new(ParticipantRefusalDetails {
                    thread_id: refusal.thread_id,
                    missing_thread_ids: Vec::new(),
                    actor: refusal.actor,
                    allowed_roles: Self::all_roles(),
                    holder: refusal.holder,
                    holder_last_seen_position: refusal.holder_last_seen_position,
                    target: refusal.target,
                    named_holder: refusal.named_holder,
                }),
            },
        }
    }
    #[must_use]
    pub fn archived_board() -> Self {
        Self::simple(
            BoardFailureKind::ArchivedBoard,
            BoardFailureStage::Admission,
            "This board is archived and read-only.",
            BoardNextAction::CorrectRequest,
        )
    }

    #[must_use]
    pub fn thread_resolved() -> Self {
        Self::simple(
            BoardFailureKind::ThreadResolved,
            BoardFailureStage::Admission,
            "This thread is resolved. Mark it unresolved before adding a thread message.",
            BoardNextAction::UnresolveThread,
        )
    }

    #[must_use]
    pub fn top_level_message_cooldown(retry_after_seconds: u64) -> Self {
        Self {
            kind: BoardFailureKind::TopLevelPostCooldown,
            stage: BoardFailureStage::Admission,
            message: format!(
                "Wait {retry_after_seconds} seconds before another top-level message in this board, or add a thread message to an existing unresolved thread."
            ),
            next_action: BoardNextAction::PostThreadPost,
            details: BoardErrorDetails::Cooldown {
                retry_after_seconds,
            },
        }
    }

    #[must_use]
    pub fn invalid_field(field: impl Into<String>, requirement: impl Into<String>) -> Self {
        let field = field.into();
        let requirement = requirement.into();
        Self {
            kind: BoardFailureKind::InvalidField,
            stage: BoardFailureStage::Validation,
            message: format!("Correct {field}: {requirement}."),
            next_action: BoardNextAction::CorrectRequest,
            details: BoardErrorDetails::FieldConstraint { field, requirement },
        }
    }

    #[must_use]
    pub fn resource_not_found(resource: ResourceIdentity) -> Self {
        Self::resource_failure(
            BoardFailureKind::ResourceNotFound,
            BoardFailureStage::Inspection,
            "The requested board resource was not found. Inspect the resource ID and selected service.",
            resource,
        )
    }

    #[must_use]
    pub fn resource_already_exists(resource: ResourceIdentity) -> Self {
        Self::resource_failure(
            BoardFailureKind::ResourceAlreadyExists,
            BoardFailureStage::Admission,
            "That resource ID already exists. Inspect the existing resource; no create effects were repeated.",
            resource,
        )
    }

    #[must_use]
    pub fn invalid_record(resource: ResourceIdentity) -> Self {
        Self::resource_failure(
            BoardFailureKind::InvalidRecord,
            BoardFailureStage::Inspection,
            "Stored board data is invalid. Inspect the identified resource.",
            resource,
        )
    }

    #[must_use]
    pub fn board_unavailable() -> Self {
        Self::simple(
            BoardFailureKind::BoardUnavailable,
            BoardFailureStage::Storage,
            "Board storage is unavailable. Retry later.",
            BoardNextAction::RetryLater,
        )
    }

    fn simple(
        kind: BoardFailureKind,
        stage: BoardFailureStage,
        message: &str,
        next_action: BoardNextAction,
    ) -> Self {
        Self {
            kind,
            stage,
            message: message.to_owned(),
            next_action,
            details: BoardErrorDetails::None,
        }
    }

    fn resource_failure(
        kind: BoardFailureKind,
        stage: BoardFailureStage,
        message: &str,
        resource: ResourceIdentity,
    ) -> Self {
        Self {
            kind,
            stage,
            message: message.to_owned(),
            next_action: BoardNextAction::InspectResource,
            details: BoardErrorDetails::Resource { resource },
        }
    }
}

impl std::fmt::Display for BoardError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for BoardError {}
