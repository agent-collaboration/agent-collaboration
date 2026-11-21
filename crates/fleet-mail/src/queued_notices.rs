//! Read and edit only the queued mailbox rows affected by notice changes.
use crate::stored_messages::decode_messages;
use central_storage::{GuardedStatement, SqlValue, Statement, StoreFailure};
use fleet_runtime_ports::stored_values::format_utc_millis;
use fleet_runtime_ports::{
    CentralStore,
    guarded_statements::{any, text},
};
use workspace_model::{ActorIdentityId, ThreadId, Timestamp, WorkspaceId};

fn notice_messages() -> Statement {
    Statement {
        sql: "SELECT message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,created_at,updated_at FROM mail_messages WHERE message_kind='notice' AND state='queued'",
        args: vec![],
    }
}
/// The caller holds the central turn for the notice plan and its applying batch.
pub async fn validate_queued_notices_held(host: &impl CentralStore) -> Result<(), StoreFailure> {
    let rows = host.read(notice_messages()).await?;
    decode_messages(&rows)?;
    Ok(())
}

/// The caller holds the central turn for validation, planning and application.
pub async fn plan_cancel_queued_notices_held(
    host: &impl CentralStore,
    workspace: &WorkspaceId,
    thread: &ThreadId,
    recipient: Option<&ActorIdentityId>,
    now: Timestamp,
) -> Result<Vec<GuardedStatement>, StoreFailure> {
    validate_queued_notices_held(host).await?;
    let recipient = recipient.map_or(SqlValue::Null, text);
    // Apply each cancellation against the transaction's current body. Two source
    // cancellations in one drain must never restore each other's removed items.
    Ok(vec![
        any(
            "UPDATE mail_messages SET body=json_set(body,'$.items',json(COALESCE((SELECT json_group_array(json(value)) FROM json_each(json_extract(mail_messages.body,'$.items')) WHERE json_extract(value,'$.workspaceId')!=? OR json_extract(value,'$.threadId') IS NOT ?),'[]'))),updated_at=? WHERE message_kind='notice' AND state='queued' AND (? IS NULL OR recipient_actor=?)",
            vec![
                text(workspace),
                text(thread),
                text(format_utc_millis(now)),
                recipient.clone(),
                recipient,
            ],
        ),
        any(
            "UPDATE mail_messages SET state=CASE WHEN json_array_length(body,'$.items')=0 THEN 'cancelled' ELSE state END,source_change_id=COALESCE(json_extract(body,'$.items[0].changeId'),source_change_id),source_workspace_id=COALESCE(json_extract(body,'$.items[0].workspaceId'),source_workspace_id),source_thread_id=CASE WHEN json_array_length(body,'$.items')=0 THEN source_thread_id ELSE json_extract(body,'$.items[0].threadId') END WHERE message_kind='notice' AND state='queued'",
            vec![],
        ),
    ])
}
