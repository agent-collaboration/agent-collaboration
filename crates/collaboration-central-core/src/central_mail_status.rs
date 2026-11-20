//! A sender reads the owning row of their direct message.
use crate::WorkspaceCaller;
use crate::{
    WorkspaceRuntime,
    central_mail_rows::encode,
    direct_message_rows::{DirectMessageStatus, decode_direct_message},
};
use central_storage::{Statement, StoreFailure};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use fleet_runtime_ports::{guarded_statements::text, workspace_decode_error::corrupt};
use workspace_ids::MessageId;
use workspace_model::{
    RefusalReason,
    central_mail::{MessageCancellation, MessageSender, MessageStatus},
};

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn direct_message_status(
        &self,
        caller: &WorkspaceCaller,
        message_id: &MessageId,
    ) -> Result<Result<MessageStatus, RefusalReason>, StoreFailure> {
        let _turn = self.execution.take_central_turn().await;
        if let Err(refusal) = self
            .access()
            .admit_workspace(
                self,
                caller.caller(),
                caller.connection().attachment_ref(),
                None,
            )
            .await?
        {
            return Ok(Err(refusal));
        }
        let Some(actor) = self.access().existing_caller_actor(caller.caller()).await? else {
            return Ok(Ok(MessageStatus::NotFound {}));
        };
        let rows = self.execution.statement_host().read(Statement {
            sql: "SELECT message_id,sender,message_kind,state,failure_reason,cancel_due_at,sender_request_id,created_at,updated_at FROM mail_messages WHERE message_id=? AND sender=? AND message_kind='direct'",
            args: vec![text(message_id), encode(&MessageSender::Actor(actor))?],
        }).await?;
        let row = match rows.rows.as_slice() {
            [] => return Ok(Ok(MessageStatus::NotFound {})),
            [row] => row,
            _ => return Err(corrupt("mail_messages.message_id")),
        };
        let status = match decode_direct_message(&rows, row)?.status {
            DirectMessageStatus::Queued => MessageStatus::Queued {},
            DirectMessageStatus::Delivered => MessageStatus::Delivered {},
            DirectMessageStatus::Failed(reason) => MessageStatus::Failed { reason },
            DirectMessageStatus::Cancelled => MessageStatus::Cancelled {
                reason: MessageCancellation::NotDeliveredWithin15Minutes,
            },
        };
        Ok(Ok(status))
    }
}
