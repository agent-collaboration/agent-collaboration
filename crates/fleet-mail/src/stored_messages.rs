//! Stored queue rows are validated before they participate in a central decision.
use central_storage::{ReadOutcome, SqlValue, Statement, StoreFailure};
use fleet_runtime_ports::CentralStore;
use fleet_runtime_ports::{
    stored_values::{column, json, string},
    workspace_decode_error::corrupt,
};
use workspace_ids::{MachineId, SessionRef};
use workspace_model::central_mail as mail;
use workspace_model::{ActorIdentityId, ChangeId, ThreadId, WorkspaceId};
fn parse<T: std::str::FromStr>(value: &str, field: &str) -> Result<T, StoreFailure> {
    value.parse().map_err(|_| corrupt(field))
}
fn optional<T: std::str::FromStr>(
    rows: &ReadOutcome,
    row: &[SqlValue],
    field: &str,
) -> Result<Option<T>, StoreFailure> {
    match column(rows, row, field)? {
        SqlValue::Null => Ok(None),
        SqlValue::Text(value) => parse(value, field).map(Some),
        _ => Err(corrupt(field)),
    }
}
pub(crate) fn decode_messages(
    rows: &ReadOutcome,
) -> Result<Vec<mail::MessageRecord>, StoreFailure> {
    rows.rows
        .iter()
        .map(|row| {
            let sender: mail::MessageSender =
                json(string(rows, row, "sender")?, "mail_messages.sender")?;
            let target: workspace_ids::SessionRef = json(
                string(rows, row, "target_session")?,
                "mail_messages.target_session",
            )?;
            let machine: MachineId = parse(
                string(rows, row, "target_machine_id")?,
                "mail_messages.target_machine_id",
            )?;
            if target.machine_id != machine {
                return Err(corrupt("mail_messages.target_machine_id"));
            }
            let body: mail::MessageBody = json(string(rows, row, "body")?, "mail_messages.body")?;
            if let mail::MessageBody::Notice { items } = &body
                && items.is_empty()
                && string(rows, row, "state")? != "cancelled"
            {
                return Err(corrupt("mail_messages.body"));
            }
            let expected_kind = crate::message_insert::stored_kind(&body);
            if string(rows, row, "message_kind")? != expected_kind {
                return Err(corrupt("mail_messages.message_kind"));
            }
            let failure = column(rows, row, "failure_reason")?;
            let state = match (string(rows, row, "state")?, failure) {
                ("queued", SqlValue::Null) => mail::MessageState::Queued,
                ("delivered", SqlValue::Null) => mail::MessageState::Delivered,
                ("cancelled", SqlValue::Null) => mail::MessageState::Cancelled,
                ("cancelled", SqlValue::Text(reason))
                    if matches!(expected_kind, "notice" | "deliveryOutcome")
                        && reason == crate::message_deadline::EXPIRED_24_HOURS =>
                {
                    mail::MessageState::Cancelled
                }
                ("failed", SqlValue::Text(reason)) if !reason.is_empty() => {
                    mail::MessageState::Failed {
                        reason: reason.clone(),
                    }
                }
                _ => return Err(corrupt("mail_messages.state")),
            };
            let change: Option<ChangeId> = optional(rows, row, "source_change_id")?;
            let recipient: Option<ActorIdentityId> = optional(rows, row, "recipient_actor")?;
            let workspace: Option<WorkspaceId> = optional(rows, row, "source_workspace_id")?;
            let thread: Option<ThreadId> = optional(rows, row, "source_thread_id")?;
            let source = match (change, recipient, workspace, thread) {
                (Some(change_id), Some(recipient_actor), Some(workspace_id), thread_id)
                    if expected_kind == "notice" =>
                {
                    Some(mail::NoticeSource {
                        change_id,
                        recipient_actor,
                        workspace_id,
                        thread_id,
                    })
                }
                (None, None, None, None) if expected_kind != "notice" => None,
                _ => return Err(corrupt("mail_messages.source_change_id")),
            };
            if let (mail::MessageBody::Notice { items }, Some(source)) = (&body, &source)
                && let Some(first) = items.first()
                && (first.change_id != source.change_id
                    || first.workspace_id != source.workspace_id
                    || first.thread_id != source.thread_id)
            {
                return Err(corrupt("mail_messages.source_change_id"));
            }
            Ok(mail::MessageRecord {
                message_id: parse(string(rows, row, "message_id")?, "mail_messages.message_id")?,
                sender,
                target,
                body,
                state,
                source,
                created_at: parse(string(rows, row, "created_at")?, "mail_messages.created_at")?,
                updated_at: parse(string(rows, row, "updated_at")?, "mail_messages.updated_at")?,
            })
        })
        .collect()
}

/// The caller holds the central turn across this non-consuming queue read.
pub async fn queued_for_session_held(
    host: &impl CentralStore,
    session: &SessionRef,
) -> Result<mail::SessionQueue, StoreFailure> {
    // UUIDv7 normally follows admission within one process. A reopen or clock
    // rollback can change that order; no FIFO promise is made inside a batch.
    let rows=host.read(Statement{sql:"SELECT message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,created_at,updated_at FROM mail_messages WHERE target_machine_id=? AND state='queued' ORDER BY message_id",args:vec![SqlValue::Text(session.machine_id.to_string())]}).await?;
    let mut messages = decode_messages(&rows)?;
    messages.retain(|message| &message.target == session);
    messages.sort_by(|left, right| left.message_id.cmp(&right.message_id));
    Ok(mail::SessionQueue {
        session: session.clone(),
        messages,
    })
}
