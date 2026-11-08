//! Sending a machine's owed socket requests within its connection's window.
//!
//! Status publications go first, then mail pushes oldest first, at most
//! [`WINDOW`] unanswered per connection, counted from the rows each time
//! (owed-work design, "Flow control"). Each send mints a fresh UUIDv7 frame id,
//! which is the send id an answer echoes. The row records the send, and the
//! frame is staged as session transport, so it leaves only after this
//! transaction commits. A subject whose owner says it is no longer owed
//! (delivered, retired, expired) loses its row here.

use super::{
    AtomicStore, ColumnIndex, CoreSessionScope, Database, Decode, Encode, IntoArguments,
    ObjectPlatform, ReborrowScope, SessionTransportEffect, StoreError, SyncTime, TransactionScope,
    Type, Uuid,
};
use crate::work_attempts::{
    AttemptKind, SocketSend,
    socket_requests::{self, RequestDestination},
};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use fleet_protocol::{FrameId, PeerId, ServerCall, ServerFrame, ServerRequest};
use runtime_primitives::{CallId, OperationId, StepName};

/// Unanswered socket requests one connection may have in flight.
pub const WINDOW: u32 = 32;

/// How many owed subjects one read returns while a send pages through them.
const PAGE: u32 = 500;

impl<Host: ObjectPlatform> CoreSessionScope<'_, Host>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'blob, Host::Database>
        + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    /// Send what `destination`'s machine is owed, within its window; answers
    /// how many requests were sent.
    pub(crate) async fn send_owed_requests(
        &mut self,
        destination: RequestDestination,
        now: SyncTime,
    ) -> Result<u32, StoreError> {
        let connection = destination.connection.as_uuid();
        let in_flight = socket_requests::in_flight_on(self, connection).await?;
        let mut free = WINDOW.saturating_sub(in_flight);
        let first_retry = now.after(crate::work_attempts::FIRST_RETRY);
        let mut sent = 0;
        // Page past other machines' rows: eligibility is per destination, so
        // no prefix of older rows for an offline machine can starve this one.
        for kind in [AttemptKind::Status, AttemptKind::Push] {
            let mut after = None;
            loop {
                let page = socket_requests::owed(self, kind, connection, after, PAGE).await?;
                let last = page.last().copied();
                for subject in page.iter().copied() {
                    if free == 0 {
                        return Ok(sent);
                    }
                    let delivered = match kind {
                        AttemptKind::Status => {
                            self.send_status(destination, subject, first_retry, now)
                                .await?
                        }
                        _ => {
                            self.send_push(destination, subject, first_retry, now)
                                .await?
                        }
                    };
                    if delivered {
                        free -= 1;
                        sent += 1;
                    }
                }
                if page.len() < PAGE as usize {
                    break;
                }
                after = last;
            }
        }
        Ok(sent)
    }

    /// An in-flight request went unanswered past its retry time: count the
    /// failure, then resend it with a new send id on its machine's current
    /// connection, or put it back to waiting (sent on the next Welcome) when the
    /// machine has none. After 24 h of failures the row is given up.
    pub(crate) async fn resend_due_request(
        &mut self,
        subject: Uuid,
        kind: AttemptKind,
        attached: &[RequestDestination],
        now: SyncTime,
    ) -> Result<(), StoreError> {
        let Some(row) = crate::work_attempts::attempt(self, subject, kind).await? else {
            return Ok(());
        };
        // A refused row's failure is already counted: an in-flight row the
        // Router refused for now, or a waiting row whose sender binding was
        // unavailable at send. Any other in-flight row went unanswered: count it.
        let refused = row.last_error.as_ref().is_some_and(|failure| {
            failure.class == crate::work_attempts::ErrorClass::RetryableRefusal
        });
        let at = match row.state {
            crate::work_attempts::AttemptState::Waiting {
                next_attempt_at: Some(at),
            } => at,
            // Already counted when refused: resend without counting again,
            // unless its failures have lasted 24 h.
            crate::work_attempts::AttemptState::InFlight {
                next_attempt_at, ..
            } if refused => {
                if row
                    .first_failed_at
                    .is_some_and(|first| now >= first.after(crate::work_attempts::RETIRE_AFTER))
                {
                    if let Some(failure) = &row.last_error {
                        crate::work_attempts::failed(self, row.id, subject, kind, failure, now)
                            .await?;
                    }
                    return Ok(());
                }
                next_attempt_at
            }
            crate::work_attempts::AttemptState::InFlight { .. } => {
                let outcome = crate::work_attempts::failed(
                    self,
                    row.id,
                    subject,
                    kind,
                    &crate::work_attempts::AttemptFailure {
                        class: crate::work_attempts::ErrorClass::Unanswered,
                        message: format!(
                            "no answer to the {} request before its retry time",
                            kind.as_str()
                        ),
                    },
                    now,
                )
                .await?;
                let crate::work_attempts::AttemptOutcome::Retry { at } = outcome else {
                    return Ok(());
                };
                at
            }
            crate::work_attempts::AttemptState::Waiting {
                next_attempt_at: None,
            }
            | crate::work_attempts::AttemptState::GivenUp => return Ok(()),
        };
        // The next retry after a refused row's due time is a backoff step away.
        let at = if at <= now {
            now.after(crate::work_attempts::backoff(row.attempts.max(1)))
        } else {
            at
        };
        let peer = self.destination_of(subject, kind, now).await?;
        let destination = peer.and_then(|peer| {
            attached
                .iter()
                .find(|destination| destination.peer == peer)
                .copied()
        });
        let resent = match (destination, kind) {
            (Some(destination), AttemptKind::Status) => {
                self.send_status(destination, subject, at, now).await?
            }
            (Some(destination), AttemptKind::Push) => {
                self.send_push(destination, subject, at, now).await?
            }
            _ => false,
        };
        if !resent
            && crate::work_attempts::attempt(self, subject, kind)
                .await?
                .is_some()
        {
            socket_requests::rewait(self, subject, kind, now).await?;
        }
        Ok(())
    }

    /// The machine an owed request goes to now, if it is still owed.
    async fn destination_of(
        &mut self,
        subject: Uuid,
        kind: AttemptKind,
        now: SyncTime,
    ) -> Result<Option<PeerId>, StoreError> {
        match kind {
            AttemptKind::Status => {
                let owed = fleet_mail_delivery::publication_by_subject::<_, BoardDeliveryTypes>(
                    self,
                    subject.to_string(),
                )
                .await
                .map_err(fleet_mail_delivery::delivery_rollback)?;
                let Some((reference, _)) = owed else {
                    return Ok(None);
                };
                crate::central_mail_sink::destination(self, &reference.target, now)
                    .await
                    .map_err(fleet_mail_delivery::delivery_rollback)
            }
            AttemptKind::Push => {
                let limits = crate::fleet_outbox_limits().map_err(StoreError::backend)?;
                let item = fleet_protocol::ItemId::new(subject).map_err(StoreError::backend)?;
                Ok(
                    fleet_mail_delivery::Outbox::<_, BoardDeliveryTypes, _>::new(
                        self,
                        limits,
                        &crate::fleet_sender_binding::FleetSenderBinding,
                    )
                    .owed_push(item, now)
                    .await
                    .map_err(fleet_mail_delivery::delivery_rollback)?
                    .map(|input| input.recipient),
                )
            }
            _ => Ok(None),
        }
    }

    /// Send one owed status publication if it goes to this machine now.
    async fn send_status(
        &mut self,
        destination: RequestDestination,
        subject: Uuid,
        retry_at: SyncTime,
        now: SyncTime,
    ) -> Result<bool, StoreError> {
        let owed = fleet_mail_delivery::publication_by_subject::<_, BoardDeliveryTypes>(
            self,
            subject.to_string(),
        )
        .await
        .map_err(fleet_mail_delivery::delivery_rollback)?;
        let Some((reference, publication)) = owed else {
            // Retired by its owner: nothing is owed any more.
            crate::work_attempts::succeeded(self, subject, AttemptKind::Status).await?;
            return Ok(false);
        };
        let goes_to = crate::central_mail_sink::destination(self, &reference.target, now)
            .await
            .map_err(fleet_mail_delivery::delivery_rollback)?;
        if goes_to != Some(destination.peer) {
            return Ok(false);
        }
        let version = reference.version.value();
        let call = call_identity(subject, &format!("status-v{version}"))?;
        self.stage_request(
            destination,
            StagedRequest {
                subject,
                kind: AttemptKind::Status,
                sent_version: Some(version),
                body: ServerCall::PublishStatus { call, publication },
            },
            retry_at,
            now,
        )
        .await?;
        Ok(true)
    }

    /// Send one owed mail push if its recipient is this machine.
    async fn send_push(
        &mut self,
        destination: RequestDestination,
        subject: Uuid,
        retry_at: SyncTime,
        now: SyncTime,
    ) -> Result<bool, StoreError> {
        let limits = crate::fleet_outbox_limits().map_err(StoreError::backend)?;
        let item = fleet_protocol::ItemId::new(subject).map_err(StoreError::backend)?;
        let mut outbox = fleet_mail_delivery::Outbox::<_, BoardDeliveryTypes, _>::new(
            self,
            limits,
            &crate::fleet_sender_binding::FleetSenderBinding,
        );
        let Some(input) = outbox
            .owed_push(item, now)
            .await
            .map_err(fleet_mail_delivery::delivery_rollback)?
        else {
            // Terminal, handed off or expired: its push is no longer owed.
            crate::work_attempts::succeeded(self, subject, AttemptKind::Push).await?;
            return Ok(false);
        };
        if input.recipient != destination.peer {
            return Ok(false);
        }
        let resolved = match outbox.resolve_push(&input.reference, now).await {
            Ok(resolved) => resolved,
            // One sender's binding being unavailable must not stop the
            // machine's other requests: refused for now, retried on backoff.
            Err(fleet_mail_delivery::DeliveryError::SenderBindingUnavailable { .. }) => {
                crate::work_attempts::failed(
                    self,
                    Uuid::now_v7(),
                    subject,
                    AttemptKind::Push,
                    &crate::work_attempts::AttemptFailure {
                        class: crate::work_attempts::ErrorClass::RetryableRefusal,
                        message: "the sender binding is unavailable".to_owned(),
                    },
                    now,
                )
                .await?;
                return Ok(false);
            }
            Err(error) => return Err(fleet_mail_delivery::delivery_rollback(error)),
        };
        let Some(resolved) = resolved else {
            crate::work_attempts::succeeded(self, subject, AttemptKind::Push).await?;
            return Ok(false);
        };
        if crate::machine_enrollment::admission::read_current_enrolled_machine(
            self,
            destination.peer,
            now,
        )
        .await?
        .is_none()
        {
            return Ok(false);
        }
        let call = call_identity(subject, "push")?;
        self.stage_request(
            destination,
            StagedRequest {
                subject,
                kind: AttemptKind::Push,
                sent_version: None,
                body: ServerCall::Push {
                    call,
                    submitter: resolved.submitter,
                    item: resolved.item,
                },
            },
            retry_at,
            now,
        )
        .await?;
        Ok(true)
    }

    /// Record the send on the row and stage its frame for after commit.
    async fn stage_request(
        &mut self,
        destination: RequestDestination,
        request: StagedRequest,
        retry_at: SyncTime,
        now: SyncTime,
    ) -> Result<(), StoreError> {
        let StagedRequest {
            subject,
            kind,
            sent_version,
            body,
        } = request;
        let send_id = Uuid::now_v7();
        let frame = FrameId::new(send_id).map_err(StoreError::backend)?;
        #[cfg(feature = "conformance")]
        let request_kind = serde_json::to_value(&body)
            .ok()
            .and_then(|value| value.get("kind")?.as_str().map(str::to_owned))
            .unwrap_or_default();
        let bytes = serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Request {
            frame,
            body: ServerRequest::Crate(body),
        })
        .map_err(StoreError::backend)?;
        let send = SocketSend {
            send_id,
            connection: destination.connection.as_uuid(),
            session: destination.session.as_uuid(),
            sent_version,
        };
        socket_requests::mark_sent(self, subject, kind, &send, retry_at, now).await?;
        // S1 binds the frame's send record to its row once the frame leaves.
        #[cfg(feature = "conformance")]
        if let Some(row) = crate::work_attempts::attempt(self, subject, kind).await? {
            crate::event_trace::bind_send(
                send_id,
                crate::event_trace::SendBinding {
                    attempt: row.id,
                    attempt_kind: kind.as_str(),
                    subject,
                    session: destination.session.as_uuid(),
                    request_kind,
                    sent_version,
                },
            );
        }
        self.transport.push(SessionTransportEffect::Frame {
            connection: destination.connection,
            bytes: bytes.into(),
        });
        Ok(())
    }
}

/// One owed request about to be sent: its row and the call it carries.
struct StagedRequest {
    subject: Uuid,
    kind: AttemptKind,
    /// The status version sent; none for a push.
    sent_version: Option<u64>,
    body: ServerCall<BoardDeliveryTypes>,
}

/// The identity a Router drops duplicates by: stable for one owed subject (and
/// one status version), unlike the per-send frame id.
fn call_identity(subject: Uuid, step: &str) -> Result<CallId, StoreError> {
    Ok(CallId {
        operation: OperationId::new(subject).map_err(StoreError::backend)?,
        step: StepName::new(step).map_err(StoreError::backend)?,
    })
}
