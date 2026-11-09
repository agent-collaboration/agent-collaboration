//! A Project's status publication goes to the Project's current Held holder.
//!
//! The real producer: the holder submits a Project item, and the recipient's
//! receipt owes the Project its status publication. The row names the Project,
//! never a machine, so each send resolves the holder afresh. No positive
//! holder answer is fabricated; receiving-store acceptance remains a higher gate.
use super::{
    composed_fleet_fixture::*,
    fleet_claim_adapter_fixture::prepare_fleet,
    fleet_mail_dispatch::{Socket, custody, envelope},
};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_protocol::{
    CallId, ClientFrame, DeliveryAnswer, DeliveryEnvelope, DeliveryItem, DeliveryRequest, FrameId,
    OperationAnswer, OperationRequest, PushResult, ServerCall, ServerFrame, ServerRequest,
    StatusCursor, StatusPublication,
};
use runtime_primitives::Clock;
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};

type Acceptance = <BoardDeliveryTypes as fleet_protocol::DeliveryTypes>::Acceptance;
type Publication = StatusPublication<Acceptance>;

/// A Project item from the holder `A` at epoch 1, addressed to `C`.
async fn submit_project_item(
    fixture: &Fixture,
) -> TestResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    let DeliveryEnvelope::Item(source) = envelope(C, SERVICE_C)? else {
        return Err("item fixture returned a cancel".into());
    };
    let item = DeliveryEnvelope::Item(DeliveryItem::new(
        source.id(),
        serde_json::from_value(json!({"kind":"project","project":PROJECT}))?,
        source.recipient().clone(),
        None,
        // A Project sends notifications only.
        serde_json::from_value(
            json!({"kind":"notification","excerpt":"Project status proof",
            "pointer":{"project":PROJECT,"subject":{"kind":"task","id":source.id()},"position":{"epoch":1,"position":1}},
            "source":{"kind":"intent","intent":durable_steps::Uuid::now_v7()}}),
        )?,
    )?);
    let answer = fixture
        .call(
            "session",
            "central.mail-submit",
            Value::Null,
            json!({
                "session":{"peer":A,"sessionId":durable_steps::Uuid::now_v7()},
                "origin":{"installation":SERVICE_A,"machine":"Project status fixture"},
                "item":item,"originEpoch":1,
            }),
        )
        .await?;
    if answer.pointer("/value/value/kind") != Some(&json!("accepted")) {
        return Err(format!("the Project item was not admitted: {answer}").into());
    }
    Ok(item)
}

/// `C` takes custody: its receipt owes the Project a status publication.
async fn recipient_accepts(
    fixture: &Fixture,
    recipient: &mut Socket,
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> TestResult {
    let frame = recipient.push(item.id()).await?;
    recipient
        .answer(
            fixture,
            frame,
            PushResult::Accepted {
                accepted_at: fixture.io.now(),
            },
        )
        .await?;
    if custody(fixture, item.id()).await?.0 != "atRecipient" {
        return Err("the recipient's receipt did not advance custody".into());
    }
    Ok(())
}

/// The owed status rows: (state, connection the last send went on).
async fn status_rows(fixture: &Fixture) -> TestResult<Vec<(String, Option<String>)>> {
    Ok(fixture
        .store
        .transaction(|scope| {
            Box::pin(async move {
                let rows = query::<sqlx_turso::Turso>(
                    "SELECT state, connection_id FROM central_work_attempts WHERE kind='status' ORDER BY subject",
                )
                .fetch_all(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                rows.iter()
                    .map(|row| {
                        Ok((
                            row.try_get("state").map_err(StoreError::backend)?,
                            row.try_get("connection_id").map_err(StoreError::backend)?,
                        ))
                    })
                    .collect()
            })
        })
        .await?)
}

/// Every queued frame on `socket`, failing on any status publication.
async fn only_notices(socket: &mut Socket) -> TestResult {
    while socket.queued_commands() > 0 {
        match socket.next().await? {
            ServerFrame::Notice { .. } => {}
            other => {
                return Err(format!(
                    "a machine without Held authority received: {}",
                    serde_json::to_string(&other)?
                )
                .into());
            }
        }
    }
    Ok(())
}

async fn publication(socket: &mut Socket) -> TestResult<(CallId, Publication)> {
    loop {
        match socket.next().await? {
            ServerFrame::Request {
                body: ServerRequest::Crate(ServerCall::PublishStatus { call, publication }),
                ..
            } => return Ok((call, publication)),
            ServerFrame::Notice { .. } => {}
            other => {
                return Err(format!(
                    "Project status received another frame: {}",
                    serde_json::to_string(&other)?
                )
                .into());
            }
        }
    }
}

async fn machine_fetch_is_empty(fixture: &Fixture, socket: &mut Socket) -> TestResult {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    let request = ClientFrame::<BoardConsumerProtocol>::Request {
        frame,
        body: OperationRequest::Delivery(DeliveryRequest::StatusFetch {
            call: serde_json::from_value(
                json!({"operation":durable_steps::Uuid::now_v7(),"step":"machine_only_status"}),
            )?,
            after: StatusCursor::new(0),
            limit: 10,
        }),
    };
    collaboration_central_core::ObjectKernel::new(&fixture.runtime)
        .on_event(collaboration_central_core::ObjectEvent::Frame {
            binding: socket.binding,
            frame: serde_json::to_vec(&request)?,
        })
        .await;
    loop {
        match socket.next().await? {
            ServerFrame::Answer {
                frame: actual,
                body: OperationAnswer::Delivery(DeliveryAnswer::StatusFetch(page)),
            } => {
                if actual != frame {
                    return Err("the fetch answered another frame".into());
                }
                if page.through != StatusCursor::new(0) || !page.events.is_empty() {
                    return Err("Machine fetch leaked the Project stream".into());
                }
                return Ok(());
            }
            ServerFrame::Notice { .. } => {}
            other => {
                return Err(format!(
                    "Machine status fetch received another frame: {}",
                    serde_json::to_string(&other)?
                )
                .into());
            }
        }
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Expired authority must keep the publication owed without routing it to the former holder or exposing Project events through Machine fetch."
)]
async fn an_expired_project_holder_keeps_the_publication_owed_without_a_frame() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), false).await?;
    let mut former = Socket::open(&fixture, A, SERVICE_A).await?;
    let mut recipient = Socket::open(&fixture, C, SERVICE_C).await?;
    let item = submit_project_item(&fixture).await?;
    // A's lease expires before C's receipt owes the Project its status.
    fixture.io.advance(60_000);
    recipient_accepts(&fixture, &mut recipient, &item).await?;
    assert_eq!(status_rows(&fixture).await?, vec![("waiting".into(), None)]);
    only_notices(&mut former).await?;
    // A due batch finds no Held holder either.
    fixture.io.advance(31_000);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    assert_eq!(status_rows(&fixture).await?, vec![("waiting".into(), None)]);
    only_notices(&mut former).await?;
    machine_fetch_is_empty(&fixture, &mut former).await?;
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "An unanswered Project publication stays owed; its resend must resolve the successor afresh under the same call."
)]
async fn a_silent_project_holder_is_resent_to_the_successor_under_the_same_call() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), false).await?;
    let mut first = Socket::open(&fixture, A, SERVICE_A).await?;
    let mut successor = Socket::open(&fixture, B, SERVICE_B).await?;
    let mut recipient = Socket::open(&fixture, C, SERVICE_C).await?;
    let item = submit_project_item(&fixture).await?;
    recipient_accepts(&fixture, &mut recipient, &item).await?;
    // The receipt's event sends the publication to the current holder, A.
    let (original, expected) = publication(&mut first).await?;
    assert_eq!(expected.item, item.id());
    assert!(original.step.as_str().starts_with("status-v"));
    only_notices(&mut successor).await?;

    let preview = fixture.owner(json!({"kind":"preview"})).await?;
    let preview = preview
        .pointer("/value/value/preview")
        .cloned()
        .ok_or("actual takeover preview absent")?;
    let answer = fixture
        .owner(json!({"kind":"takeover","value":{
            "transferId":durable_steps::Uuid::now_v7(),"toInstallation":SERVICE_B,
            "reason":"Project status retry changes its current holder","preview":preview
        }}))
        .await?;
    assert_eq!(answer.pointer("/value/kind"), Some(&json!("granted")));

    // A never answers. Past the first retry time the due batch resends it,
    // resolving the holder afresh.
    fixture.io.advance(31_000);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    let (call, sent) = publication(&mut successor).await?;
    assert_eq!(call, original);
    assert_eq!(sent, expected);
    only_notices(&mut first).await?;
    fixture.close().await
}
