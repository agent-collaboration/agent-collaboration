//! Settle one recipient acknowledgement against the owning message row.
use crate::{
    direct_message::{DirectMessage, DirectMessageRefusal, Event},
    direct_message_rows::refusal_text,
    direct_message_store::DirectMessageStore,
    message_values::timestamp,
};
use central_engine_model::{InstanceKey, TransitionError};
use central_storage::{
    AlarmUpdate, BatchOutcome, SqlValue, Statement, StatementBatch, StoreFailure,
};
use fleet_contracts::mail_link::{MailAckIgnored, MailAckSettlement};
use fleet_engine::{Engine, EngineHost, HandleOutcome};
use fleet_runtime_ports::{
    CentralStore,
    guarded_statements::{exact, text},
    stored_values::string,
    workspace_decode_error::corrupt,
};
use workspace_ids::{MachineId, MessageId};
use workspace_model::central_mail::DeliveryAnswer;

/// The caller holds the central turn and has restored live recipient authority.
/// It owns event completion after releasing the turn when settlement is applied
/// or the answer is lost.
pub async fn settle_delivery_ack_held<R: EngineHost>(
    runtime: &R,
    recipient: &MachineId,
    message_id: &MessageId,
    answer: DeliveryAnswer,
) -> Result<MailAckSettlement, StoreFailure> {
    use MailAckIgnored as Ignored;
    let ignored = |reason| Ok(MailAckSettlement::Ignored(reason));
    let rows = runtime.host().read(Statement {
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
    if &target != recipient {
        return ignored(Ignored::WrongMachine);
    }
    let kind = string(&rows, row, "message_kind")?;
    let state = string(&rows, row, "state")?;
    match (kind, state) {
        ("direct", "queued" | "cancelled") => settle_direct_ack(runtime, message_id, answer).await,
        ("notice" | "deliveryOutcome" | "wake" | "automation", "queued") => {
            settle_system_ack(runtime, message_id, &target, kind, answer).await
        }
        ("notice" | "deliveryOutcome" | "wake" | "automation", "cancelled")
        | (
            "direct" | "notice" | "deliveryOutcome" | "wake" | "automation",
            "delivered" | "failed",
        ) => ignored(Ignored::AlreadySettled),
        _ => Err(corrupt("mail_messages.state")),
    }
}

async fn settle_direct_ack<R: EngineHost>(
    runtime: &R,
    message_id: &MessageId,
    answer: DeliveryAnswer,
) -> Result<MailAckSettlement, StoreFailure> {
    let settlement = Engine::new(runtime)
        .handle_ack::<DirectMessage, _>(
            &DirectMessageStore::new(runtime),
            InstanceKey(message_id.to_string()),
            Event::Ack(answer),
        )
        .await;
    match settlement {
        HandleOutcome::Done(_) => Ok(MailAckSettlement::Applied),
        HandleOutcome::Refused(TransitionError::Domain(DirectMessageRefusal::AlreadySettled)) => {
            Ok(MailAckSettlement::Ignored(MailAckIgnored::AlreadySettled))
        }
        HandleOutcome::NotFound => Ok(MailAckSettlement::Ignored(MailAckIgnored::UnknownMessage)),
        HandleOutcome::Uncertain | HandleOutcome::Superseded => Err(StoreFailure::AnswerLost),
        HandleOutcome::StoreDown(failure) => Err(failure),
        HandleOutcome::Refused(_) | HandleOutcome::Failed(_) | HandleOutcome::RequestFailed(_) => {
            Err(corrupt("mail_messages.ack"))
        }
    }
}

async fn settle_system_ack<R: EngineHost>(
    runtime: &R,
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
                text(timestamp(runtime.now()?)),
                text(message_id),
                text(target),
                text(kind),
            ],
        )],
        alarm: AlarmUpdate::Unchanged,
    };
    match runtime.host().run_batch(batch).await? {
        BatchOutcome::Applied { .. } => Ok(MailAckSettlement::Applied),
        BatchOutcome::GuardFailed { .. } => Err(StoreFailure::AnswerLost),
        BatchOutcome::ConstraintViolated { .. } | BatchOutcome::StatementRejected { .. } => {
            Err(corrupt("mail_messages.ack"))
        }
    }
}
