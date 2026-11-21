//! Central turns and authority for the job's mailbox operations.
use crate::{WorkspaceCaller, WorkspaceRuntime};
use central_storage::StoreFailure;
use fleet_contracts::{
    MachineConnectionAttachment,
    mail_link::{
        MailAckIgnored, MailAckSettlement, WelcomeDeliveryCursor, WelcomeDeliveryPage,
        WindowedDeliveryDecision,
    },
};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use workspace_ids::{MessageId, SessionRef};
use workspace_model::{
    RefusalReason,
    central_mail::{DeliveryAnswer, MessageStatus, SessionQueue},
};

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
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
        let _turn = self.execution.take_central_turn().await;
        if connection.organization_id() != self.execution.organization()
            || self
                .restore_machine_connection_held(connection)
                .await?
                .is_err()
        {
            return Ok(MailAckSettlement::Ignored(
                MailAckIgnored::MachineAuthorityLapsed,
            ));
        }

        fleet_mail::settle_delivery_ack_held(self, connection.machine_id(), message_id, answer)
            .await
    }

    /// Read the next page of direct and system messages with an open window.
    pub async fn windowed_welcome_page(
        &self,
        attachment: &MachineConnectionAttachment,
        cursor: Option<&WelcomeDeliveryCursor>,
    ) -> Result<WelcomeDeliveryPage, StoreFailure> {
        let _turn = self.execution.take_central_turn().await;
        if self
            .restore_machine_connection_held(attachment)
            .await?
            .is_err()
        {
            return Ok(WelcomeDeliveryPage::AuthorityLapsed);
        }
        let now = self.execution.now()?;
        let page = fleet_mail::windowed_welcome_page_held(
            self.execution.statement_host(),
            attachment.machine_id(),
            cursor,
            now,
        )
        .await?;
        Ok(WelcomeDeliveryPage::Ready(page))
    }

    /// Re-observe the durable row at push time, including after a page was listed.
    pub async fn prepare_windowed_delivery(
        &self,
        attachment: &MachineConnectionAttachment,
        message_id: &MessageId,
    ) -> Result<WindowedDeliveryDecision, StoreFailure> {
        let _turn = self.execution.take_central_turn().await;
        if self
            .restore_machine_connection_held(attachment)
            .await?
            .is_err()
        {
            return Ok(WindowedDeliveryDecision::AuthorityLapsed);
        }
        fleet_mail::prepare_windowed_delivery_held(
            self,
            &self.access(),
            attachment.machine_id(),
            message_id,
        )
        .await
    }

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
        Ok(Ok(fleet_mail::direct_message_status_held(
            self.execution.statement_host(),
            &actor,
            message_id,
        )
        .await?))
    }

    /// Reads queued messages without consuming them, in stable message-UUIDv7 order.
    pub async fn queued_for_session(
        &self,
        session: &SessionRef,
    ) -> Result<SessionQueue, StoreFailure> {
        let _turn = self.execution.take_central_turn().await;
        fleet_mail::queued_for_session_held(self.execution.statement_host(), session).await
    }
}
