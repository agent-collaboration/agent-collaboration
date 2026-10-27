//! Requested notification cancellation reads authority without adding a project record.
use super::{
    AuthorityRole, BoardError, BoardErrorDetails, BoardFailureKind, BoardFailureStage,
    BoardNextAction, BoardOperation, BoardOperations, Identity, ProjectRequest, ProtocolVersion,
    RoleRequirement, RoleSubject, admission_error,
};
use collaboration_protocol::RequestedCancelResult;
use fleet_sync_protocol::{CancelBody, DeliveryCancel, DeliveryEnvelope, ItemId};

impl BoardOperations<'_> {
    pub async fn project_notification_cancel(
        &self,
        request: ProjectRequest,
    ) -> Result<RequestedCancelResult, BoardError> {
        if request.protocol_version != ProtocolVersion::CURRENT {
            return Err(BoardError::invalid_field(
                "protocolVersion",
                "unsupported project protocol",
            ));
        }
        let BoardOperation::NotificationCancel { cancel_id, target } = &request.operation else {
            return Err(BoardError::invalid_field(
                "operation",
                "notificationCancel required",
            ));
        };
        let admission = self.admission()?;
        let store = admission
            .registry
            .existing_project(&request.project_id)
            .await
            .map_err(|error| admission_error(error.into()))?
            .ok_or_else(BoardError::board_unavailable)?;
        // The native guard checks actual authority and serializes this role read.
        // No project record, request decision or follow group is appended.
        let mut read = store
            .begin_admission(admission.record_origin())
            .await
            .map_err(|error| admission_error(error.into()))?;
        let roles = read
            .active_roles(
                &RoleSubject::Project(request.project_id.clone()),
                AuthorityRole::Lead,
            )
            .await
            .map_err(|error| admission_error(error.into()))?;
        let allowed = matches!(request.actor, Identity::Human { .. })
            || roles
                .iter()
                .any(|role| role.holder.declared_identity().as_ref() == Some(&request.actor));
        if !allowed {
            read.rollback()
                .await
                .map_err(|error| admission_error(error.into()))?;
            return Err(BoardError {
                kind: BoardFailureKind::RoleRequired,
                stage: BoardFailureStage::Admission,
                message: "A project lead or the owner must request notification cancellation"
                    .into(),
                next_action: BoardNextAction::CorrectRequest,
                details: BoardErrorDetails::RoleRequired {
                    project: request.project_id,
                    actor: Box::new(request.actor),
                    requirement: RoleRequirement::Holds(AuthorityRole::Lead),
                },
            });
        }
        let original = read
            .mail_handoff(target)
            .await
            .map_err(|error| admission_error(error.into()))?
            .ok_or_else(|| BoardError::invalid_field("target", "notification handoff not found"))?;
        let message_board_storage::MailHandoffItem::Delivery(envelope) = original.item else {
            return Err(BoardError::invalid_field(
                "target",
                "notification item required",
            ));
        };
        let DeliveryEnvelope::Item(original) = *envelope else {
            return Err(BoardError::invalid_field(
                "target",
                "notification item required",
            ));
        };
        if !matches!(original.sender(),collaboration_mail_protocol::MailSender::Project {project} if project==&request.project_id)
        {
            return Err(BoardError::invalid_field(
                "target",
                "notification belongs to another project",
            ));
        }
        let epoch = read
            .admitted_epoch()
            .map_err(|error| admission_error(error.into()))?;
        let id = match cancel_id {
            Some(id) => id.clone(),
            None => uuid::Uuid::now_v7()
                .to_string()
                .try_into()
                .map_err(|_| BoardError::board_unavailable())?,
        };
        let id = ItemId::new(
            uuid::Uuid::parse_str(id.as_str())
                .map_err(|_| BoardError::invalid_field("cancelId", "UUIDv7 required"))?,
        )
        .map_err(|_| BoardError::invalid_field("cancelId", "UUIDv7 required"))?;
        let cancel = DeliveryCancel::new(
            id,
            original.sender().clone(),
            original.recipient().clone(),
            None,
            CancelBody {
                target: original.id(),
            },
        )
        .map_err(|_| BoardError::invalid_field("target", "invalid notification control"))?;
        read.rollback()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let mail = self
            .identity
            .mail_delivery
            .as_ref()
            .ok_or_else(BoardError::board_unavailable)?;
        mail.requested_cancel(
            cancel,
            collaboration_mail_protocol::CancelCause::ByRequester { by: request.actor },
            Some(epoch),
        )
        .await
        .map_err(|error| {
            tracing::warn!(%error,"requested notification cancel unavailable");
            BoardError::board_unavailable()
        })
    }
}
