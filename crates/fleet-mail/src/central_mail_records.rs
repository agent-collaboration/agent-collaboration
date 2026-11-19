//! Stored queue rows are validated before they participate in a central decision.
use crate::CentralMail;
use central_storage::{SqlValue, Statement, StoreFailure};
use fleet_contracts::central_mail_rows::decode_messages;
use fleet_contracts::{AlarmRecompute, EngineAttemptControl};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use workspace_model::central_mail as mail;
pub(crate) fn notice_messages() -> Statement {
    Statement {
        sql: "SELECT message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,created_at,updated_at FROM messages WHERE message_kind='notice' AND state='queued'",
        args: vec![],
    }
}
impl<Host: CentralStore, Stores: WorkspaceStores, Control: EngineAttemptControl + AlarmRecompute>
    CentralMail<'_, Host, Stores, Control>
{
    /// Reads queued messages without consuming them, in stable message-UUIDv7 order.
    pub async fn queued_for_session(
        &self,
        session: &workspace_model::SessionAddress,
    ) -> Result<mail::SessionQueue, StoreFailure> {
        let _turn = self.runtime.take_central_turn().await;
        // UUIDv7 normally follows admission within one process. A reopen or clock
        // rollback can change that order; no FIFO promise is made inside a batch.
        let rows=self.statement_host().read(Statement{sql:"SELECT message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,created_at,updated_at FROM messages WHERE target_machine_id=? AND state='queued' ORDER BY message_id",args:vec![SqlValue::Text(session.machine_id.to_string())]}).await?;
        let mut messages = decode_messages(&rows)?;
        messages.retain(|message| &message.target == session);
        messages.sort_by(|left, right| left.message_id.cmp(&right.message_id));
        Ok(mail::SessionQueue {
            session: session.clone(),
            messages,
        })
    }
}
