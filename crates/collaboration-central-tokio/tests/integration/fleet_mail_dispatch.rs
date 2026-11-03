//! Actual Fleet custody, dispatcher and socket correlation with an injected deadline clock.
use super::composed_fleet_fixture::{
    A, B, C, Fixture, NOW, SERVICE_A, SERVICE_B, SERVICE_C, TestResult,
};
use super::fleet_mail_inbox_fixture::RecipientInbox;
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_central_core::AdmissionOrigin;
use collaboration_central_tokio::SocketCommand;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{
    ClientAnswer, ClientFrame, ConnectionId, DeliveryEnvelope, DeliveryItem, DeliveryRefusal,
    FrameId, ItemId, PushResult, Recipient, SchemaSupport, ServerCall, ServerCallAnswer,
    ServerFrame, ServerRequest, SessionId,
};
use fleet_sync_session::{ConnectionBinding, VerifiedPeer};
use runtime_primitives::{Clock, SyncTime};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::time::Duration;
use tokio::sync::mpsc;

pub(super) const CAP: Duration = Duration::from_secs(3);
pub(super) struct Socket {
    pub(super) binding: ConnectionBinding,
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
        fixture.runtime.on_socket_opened(binding).await?;
        fixture
            .runtime
            .on_frame(
                binding,
                serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                    session_id: SessionId::new(durable_steps::Uuid::now_v7())?,
                    schema_support: SchemaSupport { max_version: 7 },
                })?,
            )
            .await?;
        let report = fixture.runtime.on_alarm().await?;
        assert!(report.failed.is_empty(), "session setup: {report:?}");
        let mut socket = Self { binding, frames };
        loop {
            if matches!(socket.next().await?, ServerFrame::Welcome { .. }) {
                break;
            }
        }
        Ok(socket)
    }
    pub(super) async fn next(&mut self) -> TestResult<ServerFrame<BoardConsumerProtocol>> {
        match tokio::time::timeout(CAP, self.frames.recv())
            .await?
            .ok_or("socket ended")?
        {
            SocketCommand::Frame(bytes) => Ok(serde_json::from_slice(&bytes)?),
            SocketCommand::Close(reason) => Err(format!("socket closed: {reason:?}").into()),
        }
    }
    pub(super) async fn push(&mut self, expected: ItemId) -> TestResult<FrameId> {
        Ok(self.push_envelope(expected).await?.0)
    }
    pub(super) async fn push_envelope(
        &mut self,
        expected: ItemId,
    ) -> TestResult<(FrameId, DeliveryEnvelope<BoardDeliveryTypes>)> {
        loop {
            if let ServerFrame::Request {
                frame,
                body: ServerRequest::Crate(ServerCall::Push { item, .. }),
            } = self.next().await?
            {
                assert_eq!(item.id(), expected);
                return Ok((frame, item));
            }
        }
    }
    pub(super) async fn answer(
        &self,
        fixture: &Fixture,
        frame: FrameId,
        answer: PushResult<<BoardDeliveryTypes as fleet_sync_protocol::DeliveryTypes>::Acceptance>,
    ) -> TestResult {
        fixture
            .runtime
            .on_frame(
                self.binding,
                serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Answer {
                    frame,
                    body: ClientAnswer::Crate(ServerCallAnswer::Pushed(answer)),
                })?,
            )
            .await?;
        Ok(())
    }
}
pub(super) fn envelope(
    peer: &str,
    service: &str,
) -> TestResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    Ok(DeliveryEnvelope::Item(DeliveryItem::new(
        ItemId::new(durable_steps::Uuid::now_v7())?,
        serde_json::from_value(json!({"kind":"humanUser","installation":SERVICE_A}))?,
        Recipient {
            peer: peer.to_owned().try_into()?,
            address: serde_json::from_value(json!({
                "endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"recipient"
            }))?,
        },
        None,
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
            "central.mail-submit-registry",
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
    let first = envelope(B, SERVICE_B)?;
    let second = envelope(C, SERVICE_C)?;
    submit(&fixture, &first).await?;
    submit(&fixture, &second).await?;
    let proof = async {
        let silent_frame = silent.push(first.id()).await?;
        responsive
            .answer(
                &fixture,
                silent_frame,
                PushResult::Accepted {
                    accepted_at: fixture.io.now(),
                },
            )
            .await?;
        let frame = responsive.push(second.id()).await?;
        responsive
            .answer(
                &fixture,
                frame,
                PushResult::Accepted {
                    accepted_at: fixture.io.now(),
                },
            )
            .await?;
        let mut deadlines = fixture.io.reply_deadlines.subscribe();
        tokio::time::timeout(CAP, deadlines.wait_for(|count| *count >= 2)).await??;
        tokio::time::timeout(CAP, async {
            loop {
                if custody(&fixture, second.id()).await? == ("atRecipient".into(), false) {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                tokio::task::yield_now().await;
            }
        })
        .await??;
        assert_eq!(custody(&fixture, first.id()).await?, ("sent".into(), true));
        fixture.io.advance(2_000);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    };
    let (report, ()) = Box::pin(tokio::time::timeout(CAP, async {
        tokio::try_join!(fixture.runtime.on_alarm(), async {
            proof.await.map_err(StoreError::backend)
        })
    }))
    .await??;
    assert!(report.failed.is_empty(), "{report:?}");
    assert_eq!(report.retried.len(), 1, "{report:?}");
    assert_eq!(custody(&fixture, first.id()).await?, ("sent".into(), true));
    assert_eq!(
        custody(&fixture, second.id()).await?,
        ("atRecipient".into(), false)
    );
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
    let inbox = RecipientInbox::open(&root.path().join("recipient"), fixture.io.clone()).await?;
    let full_limits = collaboration_central_core::central_mailbox_limits()?;
    let no_room = fleet_sync_delivery::DeliveryLimits::new::<BoardDeliveryTypes>(
        full_limits.mailbox_payload_bytes(),
        full_limits.mailbox_items(),
        full_limits.outbox_bytes(),
        full_limits.outbox_items(),
        0,
        full_limits.settlement_allowance(),
    )?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    let (report, ()) = tokio::time::timeout(CAP, async {
        tokio::try_join!(fixture.runtime.on_alarm(), async {
            let (frame, pushed) = recipient
                .push_envelope(item.id())
                .await
                .map_err(StoreError::backend)?;
            let answer = inbox
                .accept(pushed, no_room)
                .await
                .map_err(StoreError::backend)?;
            assert!(matches!(
                answer,
                PushResult::Refused {
                    reason: DeliveryRefusal::MailboxFull
                }
            ));
            recipient
                .answer(&fixture, frame, answer)
                .await
                .map_err(StoreError::backend)
        })
    })
    .await??;
    assert!(report.failed.is_empty(), "{report:?}");
    assert_eq!(report.retried.len(), 1, "{report:?}");
    assert_eq!(custody(&fixture, item.id()).await?, ("sent".into(), true));
    assert_eq!(inbox.count().await?, 0);
    fixture.io.advance(60_000);
    let (report, ()) = tokio::time::timeout(CAP, async {
        tokio::try_join!(fixture.runtime.on_alarm(), async {
            let (frame, pushed) = recipient
                .push_envelope(item.id())
                .await
                .map_err(StoreError::backend)?;
            let answer = inbox
                .accept(pushed, full_limits)
                .await
                .map_err(StoreError::backend)?;
            assert!(matches!(answer, PushResult::Accepted { .. }));
            recipient
                .answer(&fixture, frame, answer)
                .await
                .map_err(StoreError::backend)
        })
    })
    .await??;
    assert!(report.failed.is_empty(), "{report:?}");
    assert!(report.retried.is_empty(), "{report:?}");
    assert_eq!(
        custody(&fixture, item.id()).await?,
        ("atRecipient".into(), false)
    );
    assert_eq!(inbox.count().await?, 1);
    inbox.close().await?;
    fixture.close().await
}
