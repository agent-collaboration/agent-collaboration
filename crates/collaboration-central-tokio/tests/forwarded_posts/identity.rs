//! The real Core and SQL store provide a causal boundary for rejected answers.
use super::session_support::{Fixture, TestError};
use board_sync_protocol::{BoardClientAnswer, BoardConsumerProtocol, BoardPayload};
use collaboration_central_tokio::SocketCommand;
use durable_steps::{
    AtomicStore, CallId, OperationId, StepName, StoreError, Uuid, storage::TransactionScope,
};
use fleet_sync_protocol::{ClientAnswer, ClientFrame, FrameId, PeerId};
use runtime_primitives::Clock;
use serde_json::json;
use sqlx_core::{query::query, row::Row};
use std::time::Duration;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn another_verified_peer_cannot_consume_a_known_forward_identity() -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let other_root = tempfile::tempdir()?;
    let other_fixture = Fixture::open_for_installation(
        other_root.path(),
        "019f0000-0000-7000-8000-000000000004"
            .to_owned()
            .try_into()?,
    )
    .await?;
    let (holder, holder_session, mut holder_socket) = fixture.socket().await?;
    let (other, other_session, mut other_socket) = other_fixture.socket().await?;
    fixture.hello(holder, holder_session).await?;
    other_fixture.hello(other, other_session).await?;
    fixture.runtime.on_alarm().await?;
    other_fixture.runtime.on_alarm().await?;
    for socket in [&mut holder_socket, &mut other_socket] {
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(5), socket.recv()).await?,
            Some(SocketCommand::Frame(_))
        ));
    }
    let forward = FrameId::new(Uuid::now_v7())?;
    let installation = "019f0000-0000-7000-8000-000000000001";
    let sender = "019f0000-0000-7000-8000-000000000002";
    let identity = CallId {
        operation: OperationId::new(Uuid::now_v7())?,
        step: StepName::new("deliver_forward")?,
    };
    // Enter the actual delivery face to select the holder and store its identity.
    // The transport fixture records outgoing callbacks but does not run a sender.
    let delivered = fixture.runtime.delivery()?.on_call(serde_json::to_vec(&json!({
        "source":{"kind":"connection","value":sender},"sourceFace":"session",
        "authority":{"kind":"object"},
        "destination":{"kind":"connection","value":installation},"destinationFace":"delivery",
        "identity":identity,"kind":"central.deliver-forward","target":{"installation":installation},
        "request":{"forwardFrame":forward,"holder":holder.peer,
            "deadline":fixture.io.now().after(Duration::from_secs(30)),
            "post":{"project":"019f0000-0000-7000-8000-000000000003",
                "origin":{"installation":sender,"machine":"forward fixture"},
                "post":{"kind":"threadPost","body":{"secret":"never-store-forward-body"}}}}
    }))?).await?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&delivered)?,
        json!("delivered")
    );
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), holder_socket.recv()).await?,
        Some(SocketCommand::Frame(_))
    ));
    fixture
        .io
        .calls
        .lock()
        .map_err(|_| "calls mutex poisoned")?
        .clear();
    let answer = serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Answer {
        frame: forward,
        body: ClientAnswer::Consumer(BoardClientAnswer::ForwardedPost(BoardPayload::encode(
            &json!({"secret":"never-store-forward-reply"}),
        )?)),
    })?;

    other_fixture
        .io
        .calls
        .lock()
        .map_err(|_| "calls mutex poisoned")?
        .clear();
    other_fixture
        .runtime
        .on_frame(other, answer.clone())
        .await?;

    assert!(
        other_fixture
            .io
            .calls
            .lock()
            .map_err(|_| "calls mutex poisoned")?
            .is_empty(),
        "a different verified peer dispatched the holder's answer"
    );
    // Separate objects also reject an unknown correlation, so that check alone
    // cannot isolate the stored PeerId predicate. Fault the selected peer in
    // this real row, while keeping the actual session admission unchanged.
    // No normal enrollment transition is claimed by this storage-fault control.
    let original_record = replace_recorded_holder(&fixture, forward, other.peer).await?;
    fixture.runtime.on_frame(holder, answer.clone()).await?;
    assert!(
        fixture
            .io
            .calls
            .lock()
            .map_err(|_| "calls mutex poisoned")?
            .is_empty(),
        "an answer from a peer other than the recorded holder dispatched a callback"
    );
    restore_record(&fixture, forward, original_record).await?;
    fixture.runtime.on_frame(holder, answer.clone()).await?;
    {
        let calls = fixture
            .io
            .calls
            .lock()
            .map_err(|_| "calls mutex poisoned")?;
        assert_eq!(
            calls.len(),
            1,
            "the rejected peer consumed the outstanding correlation"
        );
        assert_eq!(calls[0]["kind"], "central.answer-forward");
        assert_eq!(calls[0]["request"]["forwardFrame"], json!(forward));
    }
    fixture.runtime.on_frame(holder, answer).await?;
    assert_eq!(
        fixture
            .io
            .calls
            .lock()
            .map_err(|_| "calls mutex poisoned")?
            .len(),
        1,
        "a duplicate answer dispatched a second callback"
    );
    fixture.objects.close().await?;
    other_fixture.objects.close().await?;
    Ok(())
}

async fn replace_recorded_holder(
    fixture: &Fixture,
    frame: FrameId,
    peer: PeerId,
) -> Result<String, TestError> {
    Ok(fixture
        .host
        .store
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT record FROM delivered_forwards WHERE forward_frame = ?1",
                )
                .bind(frame.to_string())
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                let original: String = row.try_get("record").map_err(StoreError::backend)?;
                let mut stored: serde_json::Value =
                    serde_json::from_str(&original).map_err(StoreError::backend)?;
                *stored.get_mut("holder").ok_or_else(|| {
                    StoreError::backend("delivered correlation lacks selected holder")
                })? = json!(peer);
                query::<sqlx_turso::Turso>(
                    "UPDATE delivered_forwards SET record = ?1 WHERE forward_frame = ?2",
                )
                .bind(stored.to_string())
                .bind(frame.to_string())
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(original)
            })
        })
        .await?)
}
async fn restore_record(
    fixture: &Fixture,
    frame: FrameId,
    record: String,
) -> Result<(), TestError> {
    fixture
        .host
        .store
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "UPDATE delivered_forwards SET record = ?1 WHERE forward_frame = ?2",
                )
                .bind(record)
                .bind(frame.to_string())
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    Ok(())
}
