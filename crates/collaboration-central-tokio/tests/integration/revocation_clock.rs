#![cfg(feature = "conformance")]
//! Enrollment time is decided after Fleet transaction entry, including time spent waiting.
use super::{
    composed_fleet_fixture::{A, Fixture, NOW, SERVICE_A, TestResult},
    fleet_mail_dispatch::Socket,
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardPayload,
    FleetRequestEnvelope,
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_protocol::{ClientFrame, FrameId, OperationAnswer, OperationRequest, ServerFrame};
use sqlx_core::query::query;
use std::future::Future;

async fn delayed_frame(advance: i64) -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut socket = Socket::open(&fixture, A, SERVICE_A).await?;
    fixture
        .store
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "UPDATE machine_enrollments SET expires_at=? WHERE machine_id=?",
                )
                .bind(
                    chrono::DateTime::from_timestamp_millis(NOW + 1)
                        .ok_or_else(|| StoreError::backend("fixture expiry"))?
                        .to_rfc3339(),
                )
                .bind(A)
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    let request = fleet_admission::FleetRequest {
        request_id: message_board::RequestId::generate(),
        workspace: durable_steps::Uuid::now_v7().to_string().try_into()?,
        actor: message_board::RecordedActor::Human {
            human_id: "clock observer".to_owned().try_into()?,
        },
        acting_for: None,
        protocol_version: message_board::ProtocolVersion::CURRENT,
        operation: fleet_admission::FleetOperation::WorkspaceCreate {
            name: "entry expiry".to_owned().try_into()?,
            description: "causal store wait".to_owned().try_into()?,
        },
    };
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    let bytes = serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
        frame,
        body: OperationRequest::Consumer(BoardConsumerRequest::Fleet(FleetRequestEnvelope {
            request_id: request.request_id.clone(),
            operation: BoardPayload::encode(&request)?,
        })),
    })?;
    let (entered, entry) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let held_store = fixture.store.clone();
    let held = tokio::spawn(async move {
        held_store
            .transaction(move |_| {
                Box::pin(async move {
                    entered
                        .send(())
                        .map_err(|_| StoreError::backend("entry observer dropped"))?;
                    released.await.map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await
    });
    entry.await?;
    let mut admission = Box::pin(fixture.runtime.on_frame(socket.binding, bytes));
    let pending = std::future::poll_fn(|context| {
        std::task::Poll::Ready(admission.as_mut().poll(context).is_pending())
    })
    .await;
    fixture.io.advance(advance);
    release.send(()).map_err(|_| "held store ended early")?;
    held.await??;
    admission.await?;
    if !pending {
        return Err("frame did not wait behind the actual Fleet transaction".into());
    }
    let answer = socket.next().await?;
    let ServerFrame::Answer {
        frame: actual,
        body:
            OperationAnswer::Consumer(BoardConsumerAnswer::Fleet {
                request_id,
                decision,
            }),
    } = answer
    else {
        return Err("unexpected frame answer".into());
    };
    if actual != frame || request_id != request.request_id {
        return Err("frame refusal lost correlation".into());
    }
    let decision: fleet_admission::FleetRequestResult = decision.decode()?;
    if advance == 0 {
        if !matches!(
            decision,
            fleet_admission::FleetRequestResult::Decided(
                message_board::RequestDecision::Accepted { .. }
            )
        ) {
            return Err("unexpired delayed-entry control failed to write".into());
        }
    } else if !matches!(
        decision,
        fleet_admission::FleetRequestResult::CallerNotEnrolled
    ) {
        return Err(
            format!("expired while waiting frame admitted shared work: {decision:?}").into(),
        );
    }
    fixture.close().await
}
#[tokio::test]
async fn frame_expiring_while_waiting_is_refused_at_equality() -> TestResult {
    delayed_frame(1).await
}
#[tokio::test]
async fn frame_expiring_while_waiting_is_refused_past_expiry() -> TestResult {
    delayed_frame(2).await
}
#[tokio::test]
async fn frame_waiting_before_expiry_still_commits_its_valid_mutation() -> TestResult {
    delayed_frame(0).await
}
