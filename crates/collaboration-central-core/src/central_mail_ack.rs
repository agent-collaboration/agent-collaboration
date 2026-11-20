//! Settle one recipient acknowledgement against the owning message row.
use crate::{
    MachineConnectionAttachment, ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls,
    central_mail_rows::timestamp,
    direct_message_rows::refusal_text,
    engine::{
        handle::HandleOutcome,
        machines::{
            direct_message::{DirectMessage, DirectMessageRefusal, Event},
            direct_message_store::DirectMessageStore,
        },
        port::Engine,
        source::{EventSource, SourcePolicy},
    },
    guarded_statements::{exact, text},
    workspace_decode_error::corrupt,
    workspace_row_codec::string,
};
use central_engine_model::{InstanceKey, Machine, TransitionError};
use central_storage::{
    AlarmUpdate, BatchOutcome, SqlValue, Statement, StatementBatch, StoreFailure,
};
use workspace_ids::{MachineId, MessageId};
use workspace_model::central_mail::DeliveryAnswer;

/// The result of applying an Ack to the central message row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailAckSettlement {
    Applied,
    Ignored(MailAckIgnored),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailAckIgnored {
    UnknownMessage,
    WrongMachine,
    AlreadySettled,
    MachineAuthorityLapsed,
}

struct AckSource;

impl<M: Machine> EventSource<M> for AckSource {
    fn policy(&self) -> SourcePolicy {
        SourcePolicy::Request
    }
}

impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub async fn acknowledge_delivery(
        &self,
        connection: &MachineConnectionAttachment,
        message_id: &MessageId,
        answer: DeliveryAnswer,
    ) -> Result<MailAckSettlement, StoreFailure> {
        let result = self
            .settle_delivery_ack(connection, message_id, answer)
            .await;
        if matches!(
            &result,
            Ok(MailAckSettlement::Applied) | Err(StoreFailure::AnswerLost)
        ) {
            let _ = self.after_event().await;
        }
        result
    }

    async fn settle_delivery_ack(
        &self,
        connection: &MachineConnectionAttachment,
        message_id: &MessageId,
        answer: DeliveryAnswer,
    ) -> Result<MailAckSettlement, StoreFailure> {
        let _turn = self.take_central_turn().await;
        use MailAckIgnored as Ignored;
        let ignored = |reason| Ok(MailAckSettlement::Ignored(reason));
        if connection.organization_id() != &self.organization
            || self
                .restore_machine_connection_held(connection)
                .await?
                .is_err()
        {
            return ignored(Ignored::MachineAuthorityLapsed);
        }

        let rows = self.host.read(Statement {
            sql: "SELECT target_machine_id,message_kind,state FROM mail_messages WHERE message_id=?",
            args: vec![text(message_id)],
        }).await?;
        let row = match rows.rows.as_slice() {
            [row] => row,
            [] => return ignored(Ignored::UnknownMessage),
            _ => return Err(corrupt("mail_messages.message_id")),
        };
        let target: MachineId = string(&rows, row, "target_machine_id")?
            .parse()
            .map_err(|_| corrupt("mail_messages.target_machine_id"))?;
        if &target != connection.machine_id() {
            return ignored(Ignored::WrongMachine);
        }
        let kind = string(&rows, row, "message_kind")?;
        let state = string(&rows, row, "state")?;
        match (kind, state) {
            ("direct", "queued" | "cancelled") => self.settle_direct_ack(message_id, answer).await,
            ("notice" | "deliveryOutcome" | "wake" | "automation", "queued") => {
                self.settle_system_ack(message_id, &target, kind, answer)
                    .await
            }
            ("notice" | "deliveryOutcome" | "wake" | "automation", "cancelled")
            | (
                "direct" | "notice" | "deliveryOutcome" | "wake" | "automation",
                "delivered" | "failed",
            ) => ignored(Ignored::AlreadySettled),
            _ => Err(corrupt("mail_messages.state")),
        }
    }

    async fn settle_direct_ack(
        &self,
        message_id: &MessageId,
        answer: DeliveryAnswer,
    ) -> Result<MailAckSettlement, StoreFailure> {
        let settlement = Engine::new(self)
            .handle::<DirectMessage, _, _>(
                &DirectMessageStore(self),
                InstanceKey(message_id.to_string()),
                Event::Ack(answer),
                &AckSource,
            )
            .await;
        match settlement {
            HandleOutcome::Done(_) => Ok(MailAckSettlement::Applied),
            HandleOutcome::Refused(TransitionError::Domain(
                DirectMessageRefusal::AlreadySettled,
            )) => Ok(MailAckSettlement::Ignored(MailAckIgnored::AlreadySettled)),
            HandleOutcome::NotFound => {
                Ok(MailAckSettlement::Ignored(MailAckIgnored::UnknownMessage))
            }
            HandleOutcome::Uncertain | HandleOutcome::Superseded => Err(StoreFailure::AnswerLost),
            HandleOutcome::StoreDown(failure) => Err(failure),
            HandleOutcome::Refused(_)
            | HandleOutcome::Failed(_)
            | HandleOutcome::RequestFailed(_) => Err(corrupt("mail_messages.ack")),
        }
    }

    async fn settle_system_ack(
        &self,
        message_id: &MessageId,
        target: &MachineId,
        kind: &str,
        answer: DeliveryAnswer,
    ) -> Result<MailAckSettlement, StoreFailure> {
        let (state, reason) = match answer {
            DeliveryAnswer::Accepted => ("delivered", SqlValue::Null),
            DeliveryAnswer::Refused { reason } => ("failed", text(refusal_text(reason)?)),
        };
        let batch = StatementBatch {
            statements: vec![exact(
                "UPDATE mail_messages SET state=?,failure_reason=?,updated_at=? WHERE message_id=? AND target_machine_id=? AND message_kind=? AND state='queued' AND failure_reason IS NULL",
                vec![
                    text(state),
                    reason,
                    text(timestamp(self.now()?)),
                    text(message_id),
                    text(target),
                    text(kind),
                ],
            )],
            alarm: AlarmUpdate::Unchanged,
        };
        match self.host.run_batch(batch).await? {
            BatchOutcome::Applied { .. } => Ok(MailAckSettlement::Applied),
            BatchOutcome::GuardFailed { .. } => Err(StoreFailure::AnswerLost),
            BatchOutcome::ConstraintViolated { .. } | BatchOutcome::StatementRejected { .. } => {
                Err(corrupt("mail_messages.ack"))
            }
        }
    }
}
