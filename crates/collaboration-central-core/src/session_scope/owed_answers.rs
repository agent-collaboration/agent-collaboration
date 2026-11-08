//! Matching a Router's answer to the owed socket request it answers.
//!
//! An answer's frame id is the send id stored on the row it answers (owed-work
//! design, "How an answer finds its row"). A holder reply still goes to the
//! reply hub until slice 4e, and a forwarded post's answer is matched first
//! (`stage_forward_answer`). Otherwise:
//! - no row holds the send id: stale (an older send, or a finished row), and
//!   the hub gets it as before;
//! - the row was sent on another connection: refused, dropped and logged;
//! - matched: the owner's result rule runs in this answer's transaction.
//!
//! Every handled answer names the machine to the event, so the slot it freed
//! is used by the event's post-event send.

use super::{
    AtomicStore, ColumnIndex, ConnectionId, CoreSessionScope, Database, Decode, Encode,
    IntoArguments, ObjectPlatform, ReborrowScope, StoreError, SyncTime, TransactionScope, Type,
    Uuid,
};
use crate::work_attempts::{
    AttemptFailure, AttemptKind, ErrorClass,
    socket_requests::{self, SentRequest},
};
use board_sync_protocol::BoardDeliveryTypes;
use fleet_protocol::{
    DeliveryRefusal, DeliveryTypes, FrameId, PushResult, ServerCallAnswer, StatusPublicationResult,
};
use fleet_session::SessionKey;

type Acceptance = <BoardDeliveryTypes as DeliveryTypes>::Acceptance;

impl<Host: ObjectPlatform> CoreSessionScope<'_, Host>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    /// Handle `answer` if it answers an owed socket request; `false` leaves it
    /// to the reply hub (a holder reply, or a stale answer).
    pub(super) async fn route_owed_answer(
        &mut self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        answer: &ServerCallAnswer<Acceptance>,
        now: SyncTime,
    ) -> Result<bool, StoreError> {
        if !matches!(
            answer,
            ServerCallAnswer::Pushed(_) | ServerCallAnswer::Published(_)
        ) {
            return Ok(false);
        }
        let Some(sent) = socket_requests::sent(self, frame.as_uuid()).await? else {
            return Ok(false);
        };
        if sent.connection != connection.as_uuid() {
            eprintln!(
                "Refused owed answer: frame {} was sent on connection {}, answered on {}",
                frame.as_uuid(),
                sent.connection,
                connection.as_uuid()
            );
            return Ok(true);
        }
        match (sent.kind, answer) {
            (AttemptKind::Push, ServerCallAnswer::Pushed(result)) => {
                self.apply_push_answer(&sent, result.clone(), now).await?;
            }
            (AttemptKind::Status, ServerCallAnswer::Published(result)) => {
                self.apply_status_answer(&sent, result).await?;
            }
            _ => {
                eprintln!(
                    "Refused owed answer: frame {} answers a {} request with another kind",
                    frame.as_uuid(),
                    sent.kind.as_str()
                );
                return Ok(true);
            }
        }
        crate::event_owed::owed_request(session.peer);
        Ok(true)
    }

    /// The push result rule: custody handed off or ended for good closes the
    /// row; a refusal for now (mailbox full, binding unavailable) is retried.
    async fn apply_push_answer(
        &mut self,
        sent: &SentRequest,
        result: PushResult<Acceptance>,
        now: SyncTime,
    ) -> Result<(), StoreError> {
        let retry = match &result {
            PushResult::BindingUnavailable { .. } => {
                Some("the recipient's sender binding is unavailable")
            }
            PushResult::Refused {
                reason: DeliveryRefusal::MailboxFull,
            } => Some("the recipient's mailbox is full"),
            _ => None,
        };
        if let Some(message) = retry {
            return self.refused_for_now(sent, message, now).await;
        }
        let limits = crate::fleet_outbox_limits().map_err(StoreError::backend)?;
        let item = fleet_protocol::ItemId::new(sent.subject).map_err(StoreError::backend)?;
        let mut outbox = fleet_mail_delivery::Outbox::<_, BoardDeliveryTypes, _>::new(
            self,
            limits,
            &crate::fleet_sender_binding::FleetSenderBinding,
        );
        if let Some(input) = outbox
            .owed_push(item, now)
            .await
            .map_err(fleet_mail_delivery::delivery_rollback)?
        {
            let receipt = outbox
                .receipt::<crate::central_mail_sink::CentralMailSink>(
                    input.recipient,
                    &input.reference,
                    result,
                    now,
                )
                .await
                .map_err(fleet_mail_delivery::delivery_rollback)?;
            if let StatusPublicationResult::Refused { reason } = receipt {
                eprintln!("Push receipt for {} refused: {reason:?}", sent.subject);
            }
        }
        crate::work_attempts::succeeded(self, sent.subject, AttemptKind::Push).await
    }

    /// The status result rule: the holder's answer for the version that was
    /// sent closes the row. Applied retires the publication; Refused ends it.
    async fn apply_status_answer(
        &mut self,
        sent: &SentRequest,
        result: &StatusPublicationResult,
    ) -> Result<(), StoreError> {
        let owed = fleet_mail_delivery::publication_by_subject::<_, BoardDeliveryTypes>(
            self,
            sent.subject.to_string(),
        )
        .await
        .map_err(fleet_mail_delivery::delivery_rollback)?;
        if let Some((reference, _)) = owed {
            // A newer version clears the send id, so a matched answer is for
            // the current version; checked again rather than assumed.
            if Some(reference.version.value()) != sent.sent_version {
                return Ok(());
            }
            if let StatusPublicationResult::Refused { reason } = result {
                eprintln!(
                    "Status publication {} v{} refused: {reason:?}",
                    sent.subject,
                    reference.version.value()
                );
            }
            fleet_mail_delivery::retire_publication::<_, BoardDeliveryTypes>(self, reference)
                .await
                .map_err(fleet_mail_delivery::delivery_rollback)?;
        }
        crate::work_attempts::succeeded(self, sent.subject, AttemptKind::Status).await
    }

    /// A machine acknowledged its status stream through these publications'
    /// events: evidence it applied them, which closes their rows like an answer.
    pub(super) async fn close_applied_statuses(
        &mut self,
        subjects: Vec<String>,
    ) -> Result<(), StoreError> {
        for subject in subjects {
            let owed = fleet_mail_delivery::publication_by_subject::<_, BoardDeliveryTypes>(
                self,
                subject.clone(),
            )
            .await
            .map_err(fleet_mail_delivery::delivery_rollback)?;
            if let Some((reference, _)) = owed {
                fleet_mail_delivery::retire_publication::<_, BoardDeliveryTypes>(self, reference)
                    .await
                    .map_err(fleet_mail_delivery::delivery_rollback)?;
            }
            let subject = Uuid::parse_str(&subject).map_err(StoreError::backend)?;
            crate::work_attempts::succeeded(self, subject, AttemptKind::Status).await?;
        }
        Ok(())
    }

    /// Count a refusal for now: the row stays in flight, holding its window
    /// slot, and the due batch resends it after the backoff (owed-work design,
    /// "How an answer finds its row": Retry).
    async fn refused_for_now(
        &mut self,
        sent: &SentRequest,
        message: &str,
        now: SyncTime,
    ) -> Result<(), StoreError> {
        socket_requests::refused_for_now(
            self,
            sent.subject,
            sent.kind,
            &AttemptFailure {
                class: ErrorClass::RetryableRefusal,
                message: message.to_owned(),
            },
            now,
        )
        .await
        .map(|_| ())
    }
}
