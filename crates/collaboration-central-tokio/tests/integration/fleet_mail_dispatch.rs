//! Actual Fleet custody, dispatcher and socket correlation with an injected deadline clock.
use super::composed_fleet_fixture::{
    A, B, C, Fixture, NOW, SERVICE_A, SERVICE_B, SERVICE_C, TestResult,
};
use super::fleet_mail_inbox_fixture::RecipientInbox;
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_central_core::AdmissionOrigin;
use collaboration_central_tokio::SocketCommand;
use durable_steps::{AtomicStore, CallId, StoreError, storage::TransactionScope};
use fleet_protocol::{
    ClientAnswer, ClientFrame, ConnectionId, DeliveryEnvelope, DeliveryItem, DeliveryRefusal,
    FrameId, ItemId, PushResult, Recipient, SchemaSupport, ServerCall, ServerCallAnswer,
    ServerFrame, ServerRequest, SessionId, Submitter,
};
use fleet_session::{ConnectionBinding, VerifiedPeer};
use runtime_primitives::{Clock, SyncTime};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::time::Duration;
use tokio::sync::mpsc;

pub(super) const CAP: Duration = Duration::from_secs(3);
pub(super) struct ReceivedPush {
    pub frame: FrameId,
    pub call: CallId,
    pub submitter: Submitter,
    pub item: DeliveryEnvelope<BoardDeliveryTypes>,
}
pub(super) struct Socket {
    pub(super) binding: ConnectionBinding,
    session_id: SessionId,
    frames: mpsc::Receiver<SocketCommand>,
}
impl Socket {
    pub(super) async fn open(fixture: &Fixture, peer: &str, service: &str) -> TestResult<Self> {
        let binding = ConnectionBinding::new(
            ConnectionId::new(durable_steps::Uuid::now_v7())?,
            VerifiedPeer {
                peer: peer.to_owned().try_into()?,
                expires_at: SyncTime::from_unix_millis(NOW + 3_600_000),
            },
        );
        let frames = fixture
            .host
            .attach_socket(
                binding,
                AdmissionOrigin {
                    installation: service.to_owned().try_into()?,
                    machine: "Fleet mail fixture".to_owned().try_into()?,
                },
            )
            .await;
        // Socket events arrive through the kernel, as the shells deliver them.
        let kernel = collaboration_central_core::ObjectKernel::new(&fixture.runtime);
        kernel
            .on_event(collaboration_central_core::ObjectEvent::SocketOpened { binding })
            .await;
        let session_id = SessionId::new(durable_steps::Uuid::now_v7())?;
        kernel
            .on_event(collaboration_central_core::ObjectEvent::Frame {
                binding,
                frame: serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                    capabilities: None,
                    session_id,
                    schema_support: SchemaSupport { max_version: 7 },
                })?,
            })
            .await;
        // The schema publication that precedes Welcome runs in an alarm.
        crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
        let mut socket = Self {
            binding,
            session_id,
            frames,
        };
        loop {
            if matches!(socket.next().await?, ServerFrame::Welcome { .. }) {
                break;
            }
        }
        Ok(socket)
    }
    pub(super) async fn fill_send_queue(&self, fixture: &Fixture) -> TestResult {
        use collaboration_central_core::ObjectPlatform;
        // Valid transport frames occupy the actual native queue; this receiver deliberately holds them.
        let frame = serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Welcome {
            capabilities: None,
            session_id: self.session_id,
            databases: Vec::new(),
        })?;
        // Fill until the host refuses a frame; the one slot left is its Close's.
        for _ in 0..self.frames.max_capacity() {
            if fixture
                .host
                .send_frame(self.binding.connection, frame.clone().into())
                .await
                .is_err()
            {
                break;
            }
        }
        if self.frames.capacity() != 1 {
            return Err("native send queue was not filled to its Close slot".into());
        }
        Ok(())
    }
    pub(super) fn frames_is_empty(&mut self) -> bool {
        self.frames.try_recv().is_err()
    }
    pub(super) async fn next(&mut self) -> TestResult<ServerFrame<BoardConsumerProtocol>> {
        match self.next_command().await? {
            SocketCommand::Frame(bytes) => Ok(serde_json::from_slice(&bytes)?),
            SocketCommand::Close(reason) => Err(format!("socket closed: {reason:?}").into()),
        }
    }
    pub(super) async fn next_command(&mut self) -> TestResult<SocketCommand> {
        Ok(tokio::time::timeout(CAP, self.frames.recv())
            .await?
            .ok_or("socket ended")?)
    }
    pub(super) async fn push(&mut self, expected: ItemId) -> TestResult<FrameId> {
        Ok(self.push_envelope(expected).await?.frame)
    }
    pub(super) fn queued_commands(&self) -> usize {
        self.frames.len()
    }
    pub(super) async fn push_envelope(&mut self, expected: ItemId) -> TestResult<ReceivedPush> {
        loop {
            if let ServerFrame::Request {
                frame,
                body:
                    ServerRequest::Crate(ServerCall::Push {
                        call,
                        submitter,
                        item,
                    }),
            } = self.next().await?
            {
                assert_eq!(item.id(), expected);
                return Ok(ReceivedPush {
                    frame,
                    call,
                    submitter,
                    item,
                });
            }
        }
    }
    pub(super) async fn answer(
        &self,
        fixture: &Fixture,
        frame: FrameId,
        answer: PushResult<<BoardDeliveryTypes as fleet_protocol::DeliveryTypes>::Acceptance>,
    ) -> TestResult {
        collaboration_central_core::ObjectKernel::new(&fixture.runtime)
            .on_event(collaboration_central_core::ObjectEvent::Frame {
                binding: self.binding,
                frame: serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Answer {
                    frame,
                    body: ClientAnswer::Crate(ServerCallAnswer::Pushed(answer)),
                })?,
            })
            .await;
        Ok(())
    }
}
pub(super) fn envelope(
    peer: &str,
    service: &str,
) -> TestResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    envelope_valid_until(peer, service, None)
}
/// An envelope whose item is valid until `valid_until`.
pub(super) fn envelope_valid_until(
    peer: &str,
    service: &str,
    valid_until: Option<SyncTime>,
) -> TestResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    Ok(DeliveryEnvelope::Item(DeliveryItem::new(
        ItemId::new(durable_steps::Uuid::now_v7())?,
        serde_json::from_value(json!({"kind":"humanUser","installation":SERVICE_A}))?,
        Recipient {
            peer: peer.to_owned().try_into()?,
            address: serde_json::from_value(json!({
                "endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"019f0000-0000-7000-8000-000000008121"
            }))?,
        },
        valid_until,
        serde_json::from_value(
            json!({"kind":"message","text":"Fleet custody proof","delivery":"queue","guard":null,"senderDisplayName":null,"replyTo":null}),
        )?,
    )?))
}
pub(super) async fn submit(
    fixture: &Fixture,
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> TestResult {
    let answer = submit_answer(fixture, item).await?;
    assert_eq!(
        answer.pointer("/value/value/kind"),
        Some(&json!("accepted")),
        "{answer}"
    );
    assert_eq!(
        answer.pointer("/value/value/value/status/kind"),
        Some(&json!("sent")),
        "{answer}"
    );
    Ok(())
}
pub(super) async fn submit_answer(
    fixture: &Fixture,
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> TestResult<Value> {
    fixture
        .call(
            "session",
            "central.mail-submit",
            Value::Null,
            json!({
                "session":{"peer":A,"sessionId":durable_steps::Uuid::now_v7()},
                "origin":{"installation":SERVICE_A,"machine":"Fleet mail fixture"},
                "item":item,"originEpoch":null,
            }),
        )
        .await
}
pub(super) async fn custody(fixture: &Fixture, item: ItemId) -> TestResult<(String, bool)> {
    Ok(fixture.store.transaction(move |scope| Box::pin(async move {
        let row = query::<sqlx_turso::Turso>("SELECT status_kind,payload IS NOT NULL AS retained FROM _fleet_sync_outbox_items WHERE item_id=?")
            .bind(item.as_uuid().to_string()).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("status_kind").map_err(StoreError::backend)?, row.try_get::<i64,_>("retained").map_err(StoreError::backend)? == 1))
    })).await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare independent recipient custody before and after the injected deadline."
)]
#[tokio::test]
async fn silent_recipient_retries_at_injected_deadline_while_another_recipient_commits()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut silent = Socket::open(&fixture, B, SERVICE_B).await?;
    let mut responsive = Socket::open(&fixture, C, SERVICE_C).await?;
    let responsive_inbox = RecipientInbox::open(
        &root.path().join("responsive"),
        fixture.io.clone(),
        C.to_owned().try_into()?,
    )
    .await?;
    let first = envelope(B, SERVICE_B)?;
    let second = envelope(C, SERVICE_C)?;
    submit(&fixture, &first).await?;
    submit(&fixture, &second).await?;
    // Each submit's own event sends the push to its connected recipient.
    let silent_frame = silent.push(first.id()).await?;
    // Adversarial input: C's claimed acceptance for B's frame arrives on the
    // wrong connection and is refused. It is not a custody receipt.
    responsive
        .answer(
            &fixture,
            silent_frame,
            PushResult::Accepted {
                accepted_at: fixture.io.now(),
            },
        )
        .await?;
    let pushed = responsive.push_envelope(second.id()).await?;
    let receipt = responsive_inbox
        .accept(
            pushed.submitter,
            pushed.item,
            collaboration_central_core::fleet_outbox_limits()?,
        )
        .await?;
    assert!(matches!(receipt, PushResult::Accepted { .. }));
    assert_eq!(responsive_inbox.count().await?, 1);
    responsive.answer(&fixture, pushed.frame, receipt).await?;
    assert_eq!(
        custody(&fixture, second.id()).await?,
        ("atRecipient".into(), false)
    );
    assert_eq!(custody(&fixture, first.id()).await?, ("sent".into(), true));
    // B never answers: past its retry time the push is sent again, same item,
    // under a new send id.
    fixture.io.advance(31_000);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    let resent = silent.push(first.id()).await?;
    assert_ne!(resent, silent_frame, "the retry reused its old send id");
    assert_eq!(custody(&fixture, first.id()).await?, ("sent".into(), true));
    assert_eq!(
        custody(&fixture, second.id()).await?,
        ("atRecipient".into(), false)
    );
    responsive_inbox.close().await?;
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions require retryable no-capacity and content release only after a receipt."
)]
#[tokio::test]
async fn no_capacity_keeps_content_and_retries_the_same_item_after_capacity_returns() -> TestResult
{
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let inbox = RecipientInbox::open(
        &root.path().join("recipient"),
        fixture.io.clone(),
        B.to_owned().try_into()?,
    )
    .await?;
    let full_limits = collaboration_central_core::fleet_outbox_limits()?;
    let no_room = fleet_mail_delivery::DeliveryLimits::new::<BoardDeliveryTypes>(
        full_limits.outbox_bytes(),
        full_limits.outbox_items(),
        0,
        full_limits.settlement_allowance(),
    )?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    // The submit's own event sends the push to the connected recipient.
    let pushed = recipient.push_envelope(item.id()).await?;
    let answer = inbox.accept(pushed.submitter, pushed.item, no_room).await?;
    assert!(matches!(
        answer,
        PushResult::Refused {
            reason: DeliveryRefusal::MailboxFull
        }
    ));
    recipient.answer(&fixture, pushed.frame, answer).await?;
    // Refused for now: the content stays and nothing is resent before the
    // 30 s backoff (owed-work design).
    assert_eq!(custody(&fixture, item.id()).await?, ("sent".into(), true));
    assert_eq!(inbox.count().await?, 0);
    assert!(
        recipient.frames_is_empty(),
        "a refused push was resent at once"
    );
    fixture.io.advance(60_000);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    let pushed = recipient.push_envelope(item.id()).await?;
    let answer = inbox
        .accept(pushed.submitter, pushed.item, full_limits)
        .await?;
    assert!(matches!(answer, PushResult::Accepted { .. }));
    recipient.answer(&fixture, pushed.frame, answer).await?;
    assert_eq!(
        custody(&fixture, item.id()).await?,
        ("atRecipient".into(), false)
    );
    assert_eq!(inbox.count().await?, 1);
    inbox.close().await?;
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The live recipient's push must be sent even behind more owed rows than one read returns."
)]
#[tokio::test]
async fn a_live_recipient_gets_its_push_behind_more_than_500_older_pushes_to_an_offline_one()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    // B never connects, so all 501 of its pushes stay owed, ahead of C's in
    // creation order: more than one 500-row read of owed subjects.
    for _ in 0..501 {
        submit(&fixture, &envelope(B, SERVICE_B)?).await?;
    }
    let mut live = Socket::open(&fixture, C, SERVICE_C).await?;
    let later = envelope(C, SERVICE_C)?;
    submit(&fixture, &later).await?;
    // The submit's own event sends it: eligibility is per destination, not a
    // prefix of the oldest owed rows.
    let frame = live.push(later.id()).await?;
    assert_ne!(frame.as_uuid(), later.id().as_uuid());
    fixture.close().await
}
