//! The real Core, requester socket and owning row expose rejected and duplicate answers.
use super::session_support::{Fixture, TestError};
use board_sync_protocol::{
    BoardClientAnswer, BoardConsumerAnswer, BoardConsumerProtocol, BoardPayload, ForwardOutcome,
};
use collaboration_central_core::AdmissionOrigin;
use collaboration_central_tokio::SocketCommand;
use durable_steps::{
    AtomicStore, CallId, OperationId, StepName, StoreError, Uuid, storage::TransactionScope,
};
use fleet_protocol::{
    ClientAnswer, ClientFrame, ConnectionId, FrameId, OperationAnswer, PeerId, ServerFrame,
    SessionId,
};
use fleet_session::{ConnectionBinding, VerifiedPeer};
use runtime_primitives::Clock;
use serde_json::json;
use sqlx_core::{query::query, row::Row};
use std::time::Duration;

#[tokio::test]
async fn another_verified_peer_cannot_consume_a_known_forward_identity() -> Result<(), TestError> {
    run(false).await
}
#[tokio::test]
async fn late_wrong_holder_and_correct_holder_answers_cannot_consume_an_expired_request()
-> Result<(), TestError> {
    run(true).await
}

async fn run(expired: bool) -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let (holder, holder_session, mut holder_socket) = fixture.socket().await?;
    fixture.hello(holder, holder_session).await?;
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    let _welcome = holder_socket.try_recv()?;
    let sender_service = "019f0000-0000-7000-8000-000000000002";
    let sender_peer = PeerId::new(Uuid::now_v7())?;
    let sender_session = SessionId::new(Uuid::now_v7())?;
    let sender = ConnectionBinding::new(
        ConnectionId::new(Uuid::now_v7())?,
        VerifiedPeer {
            peer: sender_peer,
            expires_at: fixture.io.now().after(Duration::from_secs(3600)),
        },
    );
    fixture.host.store.object.transaction(move |scope| Box::pin(async move {
        query::<sqlx_turso::Turso>("INSERT INTO machine_enrollments(machine_id,service_id,machine_label,access_client_id,credential_hash,state,expires_at,created_at,updated_at) VALUES(?,?,'forward requester','forward-client','forward-credential','enrolled','2033-05-18T03:33:20+00:00','2026-10-18T00:00:00+00:00','2026-10-18T00:00:00+00:00')")
            .bind(sender_peer.to_string()).bind(sender_service).execute(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(())
    })).await?;
    let mut sender_socket = fixture
        .host
        .native
        .attach_socket(
            sender,
            AdmissionOrigin {
                installation: sender_service.to_owned().try_into()?,
                machine: "forward requester".to_owned().try_into()?,
            },
        )
        .await;
    fixture.hello(sender, sender_session).await?;
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    let _welcome = sender_socket.try_recv()?;
    let forward = FrameId::new(Uuid::now_v7())?;
    let request_frame = FrameId::new(Uuid::now_v7())?;
    let now = fixture.io.now();
    let deadline = now.after(Duration::from_secs(30));
    fixture.host.store.object.transaction(move |scope| Box::pin(async move {
        // Arrange the registry-selected request in the same store as the actual delivery face.
        query::<sqlx_turso::Turso>("INSERT INTO machine_project_requests(id,request_frame_id,requester_machine_id,requester_session_id,requester_connection_id,project_id,holder_machine_id,phase,deadline_at,created_at,updated_at) VALUES(?,?,?,?,?,? ,?,'awaiting_holder',?,?,?)")
            .bind(forward.to_string()).bind(request_frame.to_string()).bind(sender_peer.to_string()).bind(sender_session.to_string()).bind(sender.connection.to_string())
            .bind("019f0000-0000-7000-8000-000000000003").bind(holder.peer.to_string())
            .bind(deadline.unix_millis()).bind(now.unix_millis()).bind(now.unix_millis())
            .execute(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(())
    })).await?;
    let identity = CallId {
        operation: OperationId::new(Uuid::now_v7())?,
        step: StepName::new("deliver_forward")?,
    };
    let delivered = fixture.runtime.delivery()?.on_call(serde_json::to_vec(&json!({
        "source":{"kind":"fleet"},"sourceFace":"session","authority":{"kind":"machine","value":{"installation":sender_service}},
        "destination":{"kind":"fleet"},"destinationFace":"delivery","identity":identity,
        "kind":"central.deliver-forward","target":{"installation":"019f0000-0000-7000-8000-000000000001"},
        "request":{"forwardFrame":forward,"holder":holder.peer,"deadline":deadline,
            "originSession":{"peer":sender_peer,"sessionId":sender_session},"requestFrame":request_frame,
            "connection":sender.connection,"createdAt":now,
            "post":{"project":"019f0000-0000-7000-8000-000000000003","origin":{"installation":sender_service,"machine":"forward requester"},
                "post":{"kind":"threadPost","body":{"secret":"never-store-forward-body"}}}}
    }))?).await?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&delivered)?,
        json!("delivered")
    );
    let _forward = holder_socket.try_recv()?;
    let reply = BoardPayload::encode(&json!({"secret":"never-store-forward-reply"}))?;
    let answer = serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Answer {
        frame: forward,
        body: ClientAnswer::Consumer(BoardClientAnswer::ForwardedPost(reply.clone())),
    })?;
    // Act / Assert: a real admitted requester cannot answer the holder's known frame.
    fixture.runtime.on_frame(sender, answer.clone()).await?;
    assert!(sender_socket.try_recv().is_err());
    assert_eq!(request_count(&fixture, forward).await?, 1);
    // Isolate the stored-holder predicate without changing session admission.
    let original = replace_recorded_holder(&fixture, forward, sender_peer).await?;
    fixture.runtime.on_frame(holder, answer.clone()).await?;
    assert!(sender_socket.try_recv().is_err());
    assert_eq!(request_count(&fixture, forward).await?, 1);
    replace_recorded_holder(&fixture, forward, original).await?;
    if expired {
        fixture.io.clock.set_offset(30_000);
        fixture.runtime.on_frame(sender, answer.clone()).await?;
        fixture.runtime.on_frame(holder, answer).await?;
        assert!(sender_socket.try_recv().is_err());
        assert_eq!(request_count(&fixture, forward).await?, 1);
        crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
        assert_answer(
            sender_socket.try_recv()?,
            request_frame,
            ForwardOutcome::Unanswered,
        )?;
    } else {
        fixture.runtime.on_frame(holder, answer.clone()).await?;
        assert_answer(
            sender_socket.try_recv()?,
            request_frame,
            ForwardOutcome::Answered { reply },
        )?;
        assert_eq!(request_count(&fixture, forward).await?, 0);
        fixture.runtime.on_frame(holder, answer).await?;
        assert!(
            sender_socket.try_recv().is_err(),
            "duplicate holder answer sent another requester answer"
        );
    }
    assert_eq!(request_count(&fixture, forward).await?, 0);
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Compare the real requester frame and exact terminal reply."
)]
fn assert_answer(
    command: SocketCommand,
    expected_frame: FrameId,
    expected: ForwardOutcome,
) -> Result<(), TestError> {
    let SocketCommand::Frame(bytes) = command else {
        return Err("requester received a close".into());
    };
    let ServerFrame::<BoardConsumerProtocol>::Answer {
        frame,
        body: OperationAnswer::Consumer(BoardConsumerAnswer::ForwardedPost(outcome)),
    } = serde_json::from_slice(&bytes)?
    else {
        return Err("requester received another frame".into());
    };
    assert_eq!(frame, expected_frame);
    assert_eq!(outcome, expected);
    Ok(())
}
async fn request_count(fixture: &Fixture, frame: FrameId) -> Result<i64, TestError> {
    Ok(fixture
        .host
        .store
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "SELECT count(*) AS count FROM machine_project_requests WHERE id=?",
                )
                .bind(frame.to_string())
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .try_get("count")
                .map_err(StoreError::backend)
            })
        })
        .await?)
}
async fn replace_recorded_holder(
    fixture: &Fixture,
    frame: FrameId,
    peer: PeerId,
) -> Result<PeerId, TestError> {
    Ok(fixture
        .host
        .store
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let original: String = query::<sqlx_turso::Turso>(
                    "SELECT holder_machine_id FROM machine_project_requests WHERE id=?",
                )
                .bind(frame.to_string())
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .try_get("holder_machine_id")
                .map_err(StoreError::backend)?;
                query::<sqlx_turso::Turso>(
                    "UPDATE machine_project_requests SET holder_machine_id=? WHERE id=?",
                )
                .bind(peer.to_string())
                .bind(frame.to_string())
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                original.try_into().map_err(StoreError::backend)
            })
        })
        .await?)
}
