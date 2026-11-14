//! Stored queue rows are validated before they participate in a central decision.
use crate::{
    workspace_decode_error::corrupt,
    workspace_row_codec::{column, json, string},
};
use central_storage::{ReadOutcome, SqlValue, Statement, StoreFailure};
use collaboration_protocol::central_mail as mail;
use workspace_model::{ActorIdentityId, ChangeId, MachineId, ThreadId, WorkspaceId};
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
pub(crate) fn notice_messages() -> Statement {
    Statement {
        sql: "SELECT message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,deliver_after,created_at,updated_at FROM messages WHERE message_kind='notice'",
        args: vec![],
    }
}
pub(crate) fn decode_messages(
    rows: &ReadOutcome,
) -> Result<Vec<mail::MessageRecord>, StoreFailure> {
    rows.rows
        .iter()
        .map(|row| {
            let sender: mail::MessageSender =
                json(string(rows, row, "sender")?, "messages.sender")?;
            let target: workspace_model::SessionAddress = json(
                string(rows, row, "target_session")?,
                "messages.target_session",
            )?;
            let machine: MachineId = parse(
                string(rows, row, "target_machine_id")?,
                "messages.target_machine_id",
            )?;
            if target.machine_id != machine {
                return Err(corrupt("messages.target_machine_id"));
            }
            let body: mail::MessageBody = json(string(rows, row, "body")?, "messages.body")?;
            let expected_kind = match &body {
                mail::MessageBody::Direct { .. } => "direct",
                mail::MessageBody::Notice { items } => {
                    if items.is_empty() && string(rows, row, "state")? != "cancelled" {
                        return Err(corrupt("messages.body"));
                    }
                    "notice"
                }
                mail::MessageBody::Wake { .. } => "wake",
                mail::MessageBody::Automation { .. } => "automation",
            };
            if string(rows, row, "message_kind")? != expected_kind {
                return Err(corrupt("messages.message_kind"));
            }
            let failure = column(rows, row, "failure_reason")?;
            let state = match (string(rows, row, "state")?, failure) {
                ("queued", SqlValue::Null) => mail::MessageState::Queued,
                ("ready", SqlValue::Null) => mail::MessageState::Ready,
                ("delivered", SqlValue::Null) => mail::MessageState::Delivered,
                ("cancelled", SqlValue::Null) => mail::MessageState::Cancelled,
                ("failed", SqlValue::Text(reason)) if !reason.is_empty() => {
                    mail::MessageState::Failed {
                        reason: reason.clone(),
                    }
                }
                _ => return Err(corrupt("messages.state")),
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
                _ => return Err(corrupt("messages.source_change_id")),
            };
            if let (mail::MessageBody::Notice { items }, Some(source)) = (&body, &source)
                && let Some(first) = items.first()
                && (first.change_id != source.change_id
                    || first.workspace_id != source.workspace_id
                    || first.thread_id != source.thread_id)
            {
                return Err(corrupt("messages.source_change_id"));
            }
            Ok(mail::MessageRecord {
                message_id: parse(string(rows, row, "message_id")?, "messages.message_id")?,
                sender,
                target,
                body,
                state,
                source,
                deliver_after: {
                    let due: workspace_model::Timestamp = parse(
                        string(rows, row, "deliver_after")?,
                        "messages.deliver_after",
                    )?;
                    if !crate::central_mail_time::supported(due) {
                        return Err(corrupt("messages.deliver_after"));
                    }
                    due
                },
                created_at: parse(string(rows, row, "created_at")?, "messages.created_at")?,
                updated_at: parse(string(rows, row, "updated_at")?, "messages.updated_at")?,
            })
        })
        .collect()
}

impl<Host: crate::ObjectHost, Stores: crate::WorkspaceStoreCalls>
    crate::WorkspaceRuntime<Host, Stores>
{
    /// Reads readiness without consuming it, in stable due-mark/message-id order.
    pub async fn due_for_session(
        &self,
        session: &workspace_model::SessionAddress,
        through: workspace_model::Timestamp,
    ) -> Result<mail::SessionDueBatch, StoreFailure> {
        let _turn = self.take_central_turn().await;
        if !crate::central_mail_time::supported(through) {
            return Err(corrupt("due_batch.through"));
        }
        // UUIDv7 normally follows admission within one process. A reopen or clock
        // rollback can change that order; no FIFO promise is made inside a batch.
        let rows=self.host.read(Statement{sql:"SELECT message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,deliver_after,created_at,updated_at FROM messages WHERE target_machine_id=? AND state='ready' ORDER BY deliver_after,message_id",args:vec![SqlValue::Text(session.machine_id.to_string())]}).await?;
        let mut messages = decode_messages(&rows)?;
        messages.retain(|message| &message.target == session && message.deliver_after <= through);
        messages.sort_by(|left, right| {
            left.deliver_after
                .cmp(&right.deliver_after)
                .then_with(|| left.message_id.cmp(&right.message_id))
        });
        Ok(mail::SessionDueBatch {
            session: session.clone(),
            through,
            messages,
        })
    }
}
