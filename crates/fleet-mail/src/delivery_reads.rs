//! Read and re-observe mail delivery while its recipient's central turn is held.
use crate::{
    direct_message::DirectMessage,
    direct_message_rows::failure_text,
    message_values::{encode, timestamp},
    sender_resolution::resolve_message_sender,
    stored_messages::decode_messages,
};
use central_engine_model::{InstanceKey, Machine};
use central_storage::{
    AlarmUpdate, BatchOutcome, SqlValue, Statement, StatementBatch, StoreFailure,
};
use fleet_contracts::{
    EngineSubject,
    mail_link::{
        ResolvedMailDelivery, WELCOME_PAGE_SIZE, WelcomeDeliveryCursor, WindowedDelivery,
        WindowedDeliveryDecision,
    },
};
use fleet_engine::{EngineHost, delete_attempt};
use fleet_runtime_ports::{
    CentralStore, WorkspaceStores,
    guarded_statements::{exact, text},
    stored_values::string,
    workspace_decode_error::corrupt,
};
use fleet_workspace_access::WorkspaceAccess;
use workspace_ids::{MachineId, MessageId};
use workspace_model::{
    Timestamp,
    central_mail::{MessageFailureReason, MessageRecord, MessageSender},
};

/// The caller holds the central turn and has restored live recipient authority.
pub async fn windowed_welcome_page_held(
    host: &impl CentralStore,
    recipient: &MachineId,
    cursor: Option<&WelcomeDeliveryCursor>,
    now: Timestamp,
) -> Result<Vec<WindowedDelivery>, StoreFailure> {
    let (created_at, message_id) = cursor.map_or((SqlValue::Null, SqlValue::Null), |cursor| {
        (text(timestamp(cursor.created_at)), text(&cursor.message_id))
    });
    let limit = i64::try_from(WELCOME_PAGE_SIZE)
        .ok()
        .and_then(|size| size.try_into().ok())
        .ok_or_else(|| corrupt("Welcome page size"))?;
    let rows = host.read(Statement {
        sql: "SELECT message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,created_at,updated_at,delivery_window_ends_at FROM mail_messages WHERE target_machine_id=?1 AND state='queued' AND message_kind IN ('direct','notice','deliveryOutcome') AND delivery_window_ends_at>?2 AND (?3 IS NULL OR (created_at,message_id)>(?3,?4)) ORDER BY created_at,message_id LIMIT ?5",
        args: vec![
            text(recipient), text(timestamp(now)), created_at, message_id,
            SqlValue::Integer(limit),
        ],
    }).await?;
    decode_messages(&rows)?
        .into_iter()
        .zip(&rows.rows)
        .map(|(message, row)| {
            let expires_at: Timestamp = string(&rows, row, "delivery_window_ends_at")?
                .parse()
                .map_err(|_| corrupt("mail_messages.delivery_window_ends_at"))?;
            if expires_at <= now {
                return Err(corrupt("mail_messages.delivery_window_ends_at"));
            }
            Ok(WindowedDelivery {
                message,
                expires_at,
            })
        })
        .collect()
}

/// The caller holds the central turn and has restored live recipient authority.
/// The access view must come from the same runtime as the store being read.
pub async fn prepare_windowed_delivery_held<R, S>(
    runtime: &R,
    access: &WorkspaceAccess<'_, R::Host, S>,
    recipient: &MachineId,
    message_id: &MessageId,
) -> Result<WindowedDeliveryDecision, StoreFailure>
where
    R: EngineHost,
    S: WorkspaceStores,
{
    let now = runtime.now()?;
    let rows = runtime.host().read(Statement {
        sql: "SELECT message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,created_at,updated_at,delivery_window_ends_at FROM mail_messages WHERE message_id=? AND target_machine_id=? AND state='queued' AND message_kind IN ('direct','notice','deliveryOutcome') AND delivery_window_ends_at>?",
        args: vec![text(message_id), text(recipient), text(timestamp(now))],
    }).await?;
    let mut messages = decode_messages(&rows)?;
    let row = match rows.rows.as_slice() {
        [] => return Ok(WindowedDeliveryDecision::Ineligible),
        [row] => row,
        _ => return Err(corrupt("mail_messages.message_id")),
    };
    let message = messages
        .pop()
        .ok_or_else(|| corrupt("mail_messages.message_id"))?;
    let expires_at: Timestamp = string(&rows, row, "delivery_window_ends_at")?
        .parse()
        .map_err(|_| corrupt("mail_messages.delivery_window_ends_at"))?;
    if expires_at <= now {
        return Err(corrupt("mail_messages.delivery_window_ends_at"));
    }
    let Some(sender) = resolve_message_sender(runtime.host(), access, &message).await? else {
        return fail_unresolved_sender(runtime, &message).await;
    };
    Ok(WindowedDeliveryDecision::Ready(ResolvedMailDelivery {
        message: Box::new(message),
        sender,
        expires_at,
    }))
}

/// Terminalize only the still-current direct send. There is no sender to notify.
async fn fail_unresolved_sender<R: EngineHost>(
    runtime: &R,
    message: &MessageRecord,
) -> Result<WindowedDeliveryDecision, StoreFailure> {
    let MessageSender::Actor(_) = &message.sender else {
        return Err(corrupt("mail_messages.sender"));
    };
    let now = timestamp(runtime.now()?);
    let statements = vec![
        exact(
            "UPDATE mail_messages SET state='failed',failure_reason=?,updated_at=? WHERE message_id=? AND target_machine_id=? AND sender=? AND message_kind='direct' AND state='queued' AND failure_reason IS NULL AND delivery_window_ends_at>?",
            vec![
                text(failure_text(MessageFailureReason::SenderUnresolved)?),
                text(&now),
                text(&message.message_id),
                text(&message.target.machine_id),
                encode(&message.sender)?,
                text(now),
            ],
        ),
        delete_attempt(&EngineSubject {
            kind: DirectMessage::KIND,
            key: InstanceKey(message.message_id.to_string()),
        }),
    ];
    match runtime
        .host()
        .run_batch(StatementBatch {
            statements,
            alarm: AlarmUpdate::SetEngineFromQuery {
                query: central_engine_model::next_due_sql(runtime.alarm_inputs()),
            },
        })
        .await?
    {
        BatchOutcome::Applied { .. } => Ok(WindowedDeliveryDecision::SenderUnresolved),
        BatchOutcome::GuardFailed { index: 0, .. } => Ok(WindowedDeliveryDecision::Ineligible),
        _ => Err(StoreFailure::Unavailable),
    }
}
