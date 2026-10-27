//! Typed board families route all project writes through project admission.
use super::{
    CollaborationRejection, CollaborationRejectionReason, PublishedRejection, ResultByteBudget,
};
use crate::ServiceIdentity;
use collaboration_protocol::{
    SubscriptionWaitBatch, ThreadSubscriptionPresence, ThreadSubscriptionState,
    ThreadSubscriptionView,
};
use message_board::{
    AuthorityRole, BoardError, BoardErrorDetails, BoardFailureKind, BoardFailureStage, BoardId,
    BoardNextAction, BoardOperation, Identity, PostId, ProjectId, ProjectRequest, ProtocolVersion,
    RecordKind, RecordRef, ResourceIdentity, RoleRequirement, RoleSubject, SubscriptionState,
    ThreadId, WorkspaceId,
};
use std::{sync::Arc, time::Duration};

const TARGET_PRESENCE_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
#[path = "board_adr_operations.rs"]
mod board_adr_operations;
#[path = "board_discovery_reads.rs"]
mod board_discovery_reads;
#[path = "board_entity_operations.rs"]
mod board_entity_operations;
#[path = "board_fleet_operations.rs"]
mod board_fleet_operations;
#[path = "board_fleet_pages.rs"]
mod board_fleet_pages;
#[path = "board_forwarded_posts.rs"]
mod board_forwarded_posts;
#[path = "board_inbox_events.rs"]
mod board_inbox_events;
#[path = "board_inbox_reads.rs"]
mod board_inbox_reads;
#[path = "board_non_post_refusals.rs"]
mod board_non_post_refusals;
#[path = "board_notification_cancel.rs"]
mod board_notification_cancel;
#[path = "board_paging.rs"]
mod board_paging;
#[path = "board_participant_pages.rs"]
mod board_participant_pages;
#[path = "board_post_reads.rs"]
mod board_post_reads;
#[path = "board_project_operations.rs"]
mod board_project_operations;
#[path = "board_read_marks.rs"]
mod board_read_marks;
#[path = "board_read_operations.rs"]
mod board_read_operations;
#[path = "board_receipt_pages.rs"]
mod board_receipt_pages;
#[path = "board_record_paging.rs"]
mod board_record_paging;
#[path = "board_session_context.rs"]
mod board_session_context;
#[path = "board_subscription_operations.rs"]
mod board_subscription_operations;
#[path = "board_thread_pages.rs"]
mod board_thread_pages;
#[path = "board_thread_subjects.rs"]
mod board_thread_subjects;
#[path = "board_trail_pages.rs"]
mod board_trail_pages;
#[path = "board_trail_reads.rs"]
mod board_trail_reads;
#[path = "board_view_operations.rs"]
mod board_view_operations;

pub struct BoardOperations<'service> {
    pub(super) identity: &'service ServiceIdentity,
}
impl<'service> BoardOperations<'service> {
    pub(crate) fn new(identity: &'service ServiceIdentity) -> Self {
        Self { identity }
    }
    pub(super) fn admission(&self) -> Result<&'service Arc<crate::ProjectAdmission>, BoardError> {
        self.identity
            .project_admission
            .as_ref()
            .ok_or_else(board_storage_unavailable)
    }
}
pub(super) fn admission_error(error: crate::ProjectAdmissionError) -> BoardError {
    tracing::warn!(%error,"project admission unavailable");
    match error {
        crate::ProjectAdmissionError::Storage(
            message_board_storage::ProjectStorageError::InvalidRecord { record, field },
        ) => invalid_stored_record(record, field),
        crate::ProjectAdmissionError::Fleet(
            collaboration_runtime::FleetClientError::Authoritative(
                message_board_storage::FleetStorageError::InvalidRecordAt { record },
            ),
        )
        | crate::ProjectAdmissionError::Fleet(collaboration_runtime::FleetClientError::Local(
            message_board_storage::RouterLocalStorageError::InvalidRecordAt { record },
        )) => invalid_stored_record(record, "payload"),
        crate::ProjectAdmissionError::Fleet(
            collaboration_runtime::FleetClientError::Authoritative(
                message_board_storage::FleetStorageError::InvalidRecordKey { field },
            ),
        )
        | crate::ProjectAdmissionError::Fleet(collaboration_runtime::FleetClientError::Local(
            message_board_storage::RouterLocalStorageError::InvalidRecordKey { field },
        )) => invalid_stored_field(field),
        _ => BoardError::board_unavailable(),
    }
}
fn invalid_stored_record(record: RecordRef, field: &str) -> BoardError {
    let resource = resource_identity(&record);
    resource.map_or_else(|| invalid_stored_field(field), BoardError::invalid_record)
}
fn invalid_stored_field(field: &str) -> BoardError {
    BoardError {
        kind: BoardFailureKind::InvalidRecord,
        stage: BoardFailureStage::Inspection,
        message: "Stored board data is invalid. Inspect the recorded field.".to_owned(),
        next_action: BoardNextAction::InspectResource,
        details: BoardErrorDetails::FieldConstraint {
            field: field.to_owned(),
            requirement: "must contain a valid stored domain value".to_owned(),
        },
    }
}
fn resource_identity(record: &RecordRef) -> Option<ResourceIdentity> {
    match record.kind {
        RecordKind::Workspace => WorkspaceId::try_from(record.id.as_str().to_owned())
            .ok()
            .map(|workspace_id| ResourceIdentity::Workspace { workspace_id }),
        RecordKind::Board => BoardId::try_from(record.id.as_str().to_owned())
            .ok()
            .map(|board_id| ResourceIdentity::Board { board_id }),
        RecordKind::Project => ProjectId::try_from(record.id.as_str().to_owned())
            .ok()
            .map(|project_id| ResourceIdentity::Project { project_id }),
        RecordKind::Post => PostId::try_from(record.id.as_str().to_owned())
            .ok()
            .map(|post_id| ResourceIdentity::Post { post_id }),
        RecordKind::Thread => ThreadId::try_from(record.id.as_str().to_owned())
            .ok()
            .map(|thread_id| ResourceIdentity::Thread { thread_id }),
        _ => None,
    }
}
pub(super) fn missing_record(reference: &RecordRef) -> BoardError {
    match resource_identity(reference) {
        Some(resource) => {
            let mut failure = BoardError::resource_not_found(resource);
            if reference.kind == RecordKind::Thread {
                failure.kind = BoardFailureKind::InvalidRootPost;
            }
            failure
        }
        None => BoardError::invalid_field("record", "was not found"),
    }
}
impl CollaborationRejection for BoardError {
    fn rejection_reason(&self) -> Option<CollaborationRejectionReason> {
        match self.kind {
            BoardFailureKind::Overloaded => Some(CollaborationRejectionReason::Overloaded),
            BoardFailureKind::ArchivedBoard => Some(CollaborationRejectionReason::Archived),
            BoardFailureKind::InvalidField
            | BoardFailureKind::InvalidIdentity
            | BoardFailureKind::InvalidShape => Some(CollaborationRejectionReason::InvalidShape),
            BoardFailureKind::InvalidCursor => Some(CollaborationRejectionReason::CursorInvalid),
            BoardFailureKind::TopLevelPostCooldown
            | BoardFailureKind::ReferenceTargetNotFound
            | BoardFailureKind::InvalidProjectName
            | BoardFailureKind::InvalidRootPost
            | BoardFailureKind::PositionBeyondLatest
            | BoardFailureKind::InvalidAcknowledgement
            | BoardFailureKind::NameConflict
            | BoardFailureKind::ResourceNotFound
            | BoardFailureKind::ResourceAlreadyExists
            | BoardFailureKind::OutcomeUnknown
            | BoardFailureKind::BoardUnavailable
            | BoardFailureKind::ThreadResolved
            | BoardFailureKind::InvalidRecord
            | BoardFailureKind::ParticipantRequired
            | BoardFailureKind::RoleRequired
            | BoardFailureKind::OrchestratorRequired
            | BoardFailureKind::OrchestratorAlreadyExists
            | BoardFailureKind::ImplementerAlreadyExists
            | BoardFailureKind::OrchestratorHandoverRequired
            | BoardFailureKind::HandoverTargetNotParticipant
            | BoardFailureKind::StaleOrchestrator
            | BoardFailureKind::StaleImplementer
            | BoardFailureKind::SelfReplace
            | BoardFailureKind::OrchestratorRoleChange
            | BoardFailureKind::SessionProjectPost => None,
        }
    }

    fn published_rejection(&self) -> PublishedRejection {
        PublishedRejection::typed(
            PublishedRejection::OPERATION_FAILED,
            self.message.clone(),
            self,
        )
    }
}

/// Why a response bound leaves no room for a thread-wait notice.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ThreadWaitBudgetError {
    #[error("the response bound leaves no room for a subscription wait response")]
    NoRoomForResponse,
    #[error("the response bound leaves no room for one subscription root")]
    NoRoomForOneRoot,
}

/// The bytes a wait notice's roots may take so the largest notice still fits `budget`.
///
/// The notice reserves room for the longest push line and held-since time; only its roots
/// shrink to fit.
pub fn thread_wait_root_notice_limit(
    budget: ResultByteBudget,
) -> Result<usize, ThreadWaitBudgetError> {
    const MAX_HELD_SINCE_WIRE_BYTES: usize = 64;
    let empty_roots = serde_json::json!([]);
    let largest_notice = serde_json::json!({
        "renewals":[],
        "outcome":{"kind":"batch","batch":{
            "kind":"notice",
            "pushId":"01890f2e-7b4c-7cc0-98c4-000000000002",
            "line":"\\".repeat(collaboration_protocol::MAX_PUSH_LINE_BYTES),
            "held":true,
            "heldSince":"x".repeat(MAX_HELD_SINCE_WIRE_BYTES),
            "draining":true,
            "roots":empty_roots,
        }}
    });
    let empty_roots_bytes = serde_json::to_vec(&empty_roots)
        .map_err(|_| ThreadWaitBudgetError::NoRoomForResponse)?
        .len();
    let fixed_response_bytes = budget
        .response_bytes(&largest_notice)
        .and_then(|bytes| bytes.checked_sub(empty_roots_bytes))
        .ok_or(ThreadWaitBudgetError::NoRoomForResponse)?;
    let maximum_root_notice_bytes = budget
        .response_limit_bytes()
        .checked_sub(fixed_response_bytes)
        .ok_or(ThreadWaitBudgetError::NoRoomForResponse)?;
    let largest_root = serde_json::json!([{
        "threadId":"01890f2e-7b4c-7cc0-98c4-000000000002",
        "projectId":"01890f2e-7b4c-7cc0-98c4-000000000003",
        "from":{"epoch":i64::MAX,"position":i64::MAX},
        "through":{"epoch":i64::MAX,"position":i64::MAX},
        "postCount":u64::MAX,
    }]);
    let minimum_root_notice_bytes = serde_json::to_vec(&largest_root)
        .map_err(|_| ThreadWaitBudgetError::NoRoomForOneRoot)?
        .len();
    if maximum_root_notice_bytes < minimum_root_notice_bytes {
        return Err(ThreadWaitBudgetError::NoRoomForOneRoot);
    }
    Ok(maximum_root_notice_bytes)
}

/// The board store is not open on this Router.
fn board_storage_unavailable() -> BoardError {
    BoardError {
        kind: BoardFailureKind::BoardUnavailable,
        stage: BoardFailureStage::Admission,
        message: "Board storage unavailable; inspect the selected debug/service profile.".into(),
        next_action: BoardNextAction::RetryLater,
        details: BoardErrorDetails::None,
    }
}

fn subscription_wait_result(
    result: Option<crate::SubscriptionWaitResult>,
) -> collaboration_protocol::ThreadSubscriptionWaitOutcome {
    let batch = result.map(|result| match result {
        crate::SubscriptionWaitResult::Notice {
            push_id,
            line,
            batch,
        } => SubscriptionWaitBatch::Notice {
            push_id,
            line,
            held: batch.held,
            held_since: batch.held_since,
            draining: batch.draining,
            roots: batch.roots,
        },
        crate::SubscriptionWaitResult::Ranges { batch } => SubscriptionWaitBatch::Ranges {
            held: batch.held,
            held_since: batch.held_since,
            draining: batch.draining,
            roots: batch.roots,
        },
    });
    match batch {
        Some(batch) => collaboration_protocol::ThreadSubscriptionWaitOutcome::Batch { batch },
        None => collaboration_protocol::ThreadSubscriptionWaitOutcome::TimedOut {},
    }
}

fn subscription_view(
    row: crate::service_identity::subscription_delivery::ProjectSubscriptionRecord,
    presence: ThreadSubscriptionPresence,
) -> ThreadSubscriptionView {
    let record = row.record;
    let state = match record.state() {
        SubscriptionState::Active => ThreadSubscriptionState::Active,
        SubscriptionState::Draining => ThreadSubscriptionState::Draining,
        SubscriptionState::Ended { .. } => ThreadSubscriptionState::Ended,
    };
    let pending_count = record
        .roots()
        .iter()
        .map(|root| root.pending_count())
        .sum::<u64>()
        .saturating_add(row.delivery.allocated_pending);
    let held_since = row.delivery.held_since;
    let next_retry_at = row.delivery.next_retry_at;
    ThreadSubscriptionView {
        project_id: row.project,
        scope: record.scope().clone(),
        policy: record.policy().clone(),
        state,
        end_reason: record.state().end_reason(),
        expires_at: record.expires_at(),
        pending_count,
        presence,
        held_since,
        next_retry_at,
        last_outcome: row
            .delivery
            .last_outcome
            .or_else(|| record.last_outcome().cloned()),
    }
}

async fn presence_for_reader(
    reader: &Identity,
    presence_probe: &Arc<dyn collaboration_contracts::TargetPresenceProbe>,
) -> ThreadSubscriptionPresence {
    let Identity::Session { session } = reader else {
        return ThreadSubscriptionPresence::Unreachable {
            reason: "no session target".to_owned(),
        };
    };
    let target = match serde_json::to_value(session)
        .and_then(serde_json::from_value::<collaboration_protocol::SessionRef>)
    {
        Ok(target) => target,
        Err(error) => {
            tracing::warn!(error=%error,"subscription target identity could not be projected for presence");
            return ThreadSubscriptionPresence::Unreachable {
                reason: "presence unavailable".to_owned(),
            };
        }
    };
    match tokio::time::timeout(
        TARGET_PRESENCE_PROBE_TIMEOUT,
        presence_probe.presence(&target),
    )
    .await
    {
        Ok(Ok(collaboration_contracts::TargetPresence::Running)) => {
            ThreadSubscriptionPresence::Running {}
        }
        Ok(Ok(collaboration_contracts::TargetPresence::Wakeable)) => {
            ThreadSubscriptionPresence::Wakeable {}
        }
        Ok(Ok(collaboration_contracts::TargetPresence::Unreachable { reason })) => {
            ThreadSubscriptionPresence::Unreachable { reason }
        }
        Ok(Err(error)) => {
            tracing::warn!(error=%error,"subscription target presence probe failed");
            ThreadSubscriptionPresence::Unreachable {
                reason: "presence unavailable".to_owned(),
            }
        }
        Err(_elapsed) => {
            tracing::warn!(
                timeout_seconds = TARGET_PRESENCE_PROBE_TIMEOUT.as_secs(),
                "subscription target presence probe timed out"
            );
            ThreadSubscriptionPresence::Unreachable {
                reason: "presence unavailable".to_owned(),
            }
        }
    }
}
