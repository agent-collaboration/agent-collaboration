#![cfg(feature = "conformance")]

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "session_publication/support.rs"]
mod support;

use collaboration_central_core::ObjectHost;
use collaboration_central_tokio::SocketCommand;
use durable_steps::{CallFailure, CallId, OperationId, StepName, Uuid};
use fleet_sync_protocol::{FrameId, PeerId, ReleaseAnswer, ReleaseRequest, SessionId};
use fleet_sync_session::{ConnectionBinding, ConnectionRegistration};
use serde_json::{Value, json};
use std::{sync::atomic::Ordering, time::Duration};
use support::{Fixture, TestError};
use tokio::{sync::mpsc, task::JoinHandle};

fn identity() -> Result<CallId, TestError> {
    Ok(CallId {
        operation: OperationId::new(Uuid::now_v7())?,
        step: StepName::new("holder_request")?,
    })
}
fn request() -> Result<ReleaseRequest, TestError> {
    Ok(serde_json::from_value(
        json!({"database":"019f0000-0000-7000-8000-000000000503","epoch":1,"transfer":"019f0000-0000-7000-8000-000000000504"}),
    )?)
}
fn start(
    fixture: &Fixture,
    peer: PeerId,
    identity: CallId,
) -> Result<JoinHandle<Result<ReleaseAnswer, CallFailure>>, TestError> {
    let runtime = fixture.runtime.clone();
    let request = request()?;
    Ok(tokio::spawn(async move {
        runtime
            .conformance_holder_release(peer, request, identity)
            .await
    }))
}
async fn frame(socket: &mut mpsc::Receiver<SocketCommand>) -> Result<Value, TestError> {
    match tokio::time::timeout(Duration::from_secs(5), socket.recv()).await? {
        Some(SocketCommand::Frame(bytes)) => Ok(serde_json::from_slice(&bytes)?),
        _ => Err("holder frame missing".into()),
    }
}
fn frame_id(frame: &Value) -> Result<FrameId, TestError> {
    Ok(serde_json::from_value(
        frame
            .pointer("/value/frame")
            .cloned()
            .ok_or("frame id missing")?,
    )?)
}
async fn answer(
    fixture: &Fixture,
    binding: ConnectionBinding,
    frame: FrameId,
) -> Result<(), TestError> {
    fixture.runtime.on_frame(binding, serde_json::to_vec(&json!({"kind":"answer","value":{"frame":frame,"body":{"kind":"crate","value":{"kind":"release","value":{"kind":"released","value":{"final":{"epoch":1,"sequence":0}}}}}}}))?).await?;
    Ok(())
}
async fn attached(
    fixture: &Fixture,
    peer: Option<PeerId>,
) -> Result<(ConnectionBinding, SessionId, mpsc::Receiver<SocketCommand>), TestError> {
    let (binding, session, mut socket) = match peer {
        Some(peer) => fixture.socket_for(peer).await?,
        None => fixture.socket().await?,
    };
    fixture.hello(binding, session).await?;
    fixture.runtime.on_alarm().await?;
    assert_eq!(
        frame(&mut socket).await?,
        json!({"kind":"welcome","value":{"sessionId":session,"databases":[]}})
    );
    Ok((binding, session, socket))
}
async fn completed(
    pending: JoinHandle<Result<ReleaseAnswer, CallFailure>>,
) -> Result<(), TestError> {
    let answer = tokio::time::timeout(Duration::from_secs(5), pending).await???;
    assert_eq!(
        serde_json::to_value(answer)?,
        json!({"kind":"released","value":{"final":{"epoch":1,"sequence":0}}})
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn selection_skips_opening_attaching_closed_and_expired_sockets_then_uses_a_healthy_replacement()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let (binding, session, mut socket) = fixture.socket().await?;
    for expected in [
        ConnectionRegistration::Opening,
        ConnectionRegistration::Attaching {
            session_id: session,
            schema_support: fleet_sync_protocol::SchemaSupport { max_version: 7 },
            attach_seq: fleet_sync_protocol::AttachSequence::new(1)?,
        },
    ] {
        assert_eq!(
            fixture.host.registrations().await?.get(&binding.connection),
            Some(&expected)
        );
        let refused = fixture
            .runtime
            .conformance_holder_release(binding.peer, request()?, identity()?)
            .await;
        assert!(matches!(refused, Err(CallFailure::NotSent { .. })));
        assert!(matches!(
            socket.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        if expected == ConnectionRegistration::Opening {
            fixture.hello(binding, session).await?;
        }
    }
    fixture.runtime.on_alarm().await?;
    let _welcome = frame(&mut socket).await?;
    fixture
        .io
        .clock_offset_millis
        .store(3_600_000, Ordering::SeqCst);
    assert!(matches!(
        fixture
            .runtime
            .conformance_holder_release(binding.peer, request()?, identity()?)
            .await,
        Err(CallFailure::NotSent { .. })
    ));
    fixture.io.clock_offset_millis.store(0, Ordering::SeqCst);
    fixture
        .host
        .publish_registrations([(binding.connection, ConnectionRegistration::Closed)].into())
        .await;
    // Identity remains readable: refusal must come from readiness, not physical removal.
    assert_eq!(
        fixture.host.admission_binding(binding.connection).await?,
        binding
    );
    assert!(matches!(
        fixture
            .runtime
            .conformance_holder_release(binding.peer, request()?, identity()?)
            .await,
        Err(CallFailure::NotSent { .. })
    ));
    let (replacement, _, mut replacement_socket) = attached(&fixture, Some(binding.peer)).await?;
    assert_eq!(
        frame(&mut socket).await?,
        json!({"kind":"closed","value":{"reason":{"kind":"sessionEnded"}}})
    );
    assert!(matches!(
        socket.try_recv(),
        Ok(SocketCommand::Close(
            fleet_sync_protocol::CloseReason::SessionEnded
        ))
    ));
    let pending = start(&fixture, binding.peer, identity()?)?;
    let sent = frame(&mut replacement_socket).await?;
    answer(&fixture, replacement, frame_id(&sent)?).await?;
    completed(pending).await?;
    assert!(matches!(
        socket.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn separate_answer_events_require_the_exact_connection_and_frame_without_holding_the_local_section()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let (holder, _, mut socket) = attached(&fixture, None).await?;
    let other_directory = tempfile::tempdir()?;
    let other_fixture = Fixture::open_for_installation(
        other_directory.path(),
        "019f0000-0000-7000-8000-000000000002"
            .to_owned()
            .try_into()?,
    )
    .await?;
    let (other, _, _other_socket) = attached(&other_fixture, None).await?;
    let pending = start(&fixture, holder.peer, identity()?)?;
    let sent = frame(&mut socket).await?;
    let frame = frame_id(&sent)?;
    // Both answers pass real shared frame/session validation. The hub must still
    // distinguish their actual attachment and frame identities.
    tokio::time::timeout(Duration::from_secs(5), answer(&other_fixture, other, frame)).await??;
    assert_eq!(
        fixture.runtime.conformance_holder_observation()?.0,
        1,
        "another valid connection consumed the holder's answer"
    );
    answer(&fixture, holder, FrameId::new(Uuid::now_v7())?).await?;
    assert_eq!(
        fixture.runtime.conformance_holder_observation()?.0,
        1,
        "another frame consumed the holder's answer"
    );
    tokio::time::timeout(Duration::from_secs(5), answer(&fixture, holder, frame)).await??;
    completed(pending).await?;
    fixture.objects.close().await?;
    other_fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn close_and_the_existing_alarm_fail_sent_requests_as_answer_lost() -> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let peer = PeerId::new(Uuid::now_v7())?;
    for close in [true, false] {
        let (holder, _, mut socket) = attached(&fixture, Some(peer)).await?;
        let pending = start(&fixture, holder.peer, identity()?)?;
        let _sent = frame(&mut socket).await?;
        if close {
            fixture.runtime.on_socket_closed(holder).await?;
        } else {
            fixture
                .io
                .clock_offset_millis
                .store(2_000, Ordering::SeqCst);
            fixture.runtime.on_alarm().await?;
        }
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(5), pending).await??,
            Err(CallFailure::AnswerLost)
        ));
        fixture.io.clock_offset_millis.store(0, Ordering::SeqCst);
    }
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn dropping_a_wait_retries_the_same_call_with_a_fresh_frame_and_drops_the_late_answer()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let (holder, _, mut socket) = attached(&fixture, None).await?;
    let call = identity()?;
    let first = start(&fixture, holder.peer, call.clone())?;
    let lost = frame(&mut socket).await?;
    first.abort();
    assert!(first.await.is_err_and(|error| error.is_cancelled()));
    let retry = start(&fixture, holder.peer, call)?;
    let resent = frame(&mut socket).await?;
    assert_eq!(
        lost.pointer("/value/body/value/value/call"),
        resent.pointer("/value/body/value/value/call")
    );
    assert_ne!(frame_id(&lost)?, frame_id(&resent)?);
    answer(&fixture, holder, frame_id(&lost)?).await?;
    assert_eq!(
        fixture.runtime.conformance_holder_observation()?.0,
        1,
        "a late old frame consumed the retry"
    );
    answer(&fixture, holder, frame_id(&resent)?).await?;
    completed(retry).await?;
    fixture.objects.close().await?;
    Ok(())
}

// Decision533 retires the old multi-peer/single-installation overlap setup.
// Library session_rules.rs:232-239 closes the previous connection before new
// schema publication; EffectDriver::Close fails its exact-bound waiter.
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn replacing_the_holder_during_schema_publication_loses_the_old_attempt_and_retries_on_the_new_socket()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let (holder, session, mut old_socket) = attached(&fixture, None).await?;
    let call = identity()?;
    let pending = start(&fixture, holder.peer, call.clone())?;
    let original = frame(&mut old_socket).await?;
    let old_frame = frame_id(&original)?;
    let (replacement, _, mut new_socket) = fixture.socket_for(holder.peer).await?;
    fixture.hello(replacement, session).await?;
    fixture.io.schema.hold.store(true, Ordering::SeqCst);
    let runtime = fixture.runtime.clone();
    let publication = tokio::spawn(async move { runtime.on_alarm().await });
    tokio::time::timeout(Duration::from_secs(5), fixture.io.schema.entered.acquire())
        .await??
        .forget();

    assert_eq!(
        frame(&mut old_socket).await?,
        json!({"kind":"closed","value":{"reason":{"kind":"connectionReplaced"}}})
    );
    assert!(matches!(
        old_socket.try_recv(),
        Ok(SocketCommand::Close(
            fleet_sync_protocol::CloseReason::ConnectionReplaced
        ))
    ));
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), pending).await??,
        Err(CallFailure::AnswerLost)
    ));
    assert!(!publication.is_finished());
    assert!(
        matches!(
            fixture
                .runtime
                .conformance_holder_release(holder.peer, request()?, call.clone())
                .await,
            Err(CallFailure::NotSent { .. })
        ),
        "an attaching replacement received a holder request"
    );
    assert!(matches!(
        new_socket.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    fixture.io.schema.hold.store(false, Ordering::SeqCst);
    fixture.io.schema.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), publication).await???;
    assert_eq!(
        frame(&mut new_socket).await?,
        json!({"kind":"welcome","value":{"sessionId":session,"databases":[]}})
    );

    let retry = start(&fixture, holder.peer, call)?;
    let resent = frame(&mut new_socket).await?;
    assert_eq!(
        original.pointer("/value/body/value/value/call"),
        resent.pointer("/value/body/value/value/call")
    );
    assert_ne!(old_frame, frame_id(&resent)?);
    answer(&fixture, replacement, old_frame).await?;
    assert_eq!(
        fixture.runtime.conformance_holder_observation()?.0,
        1,
        "the old frame consumed the replacement's attempt"
    );
    answer(&fixture, replacement, frame_id(&resent)?).await?;
    completed(retry).await?;
    answer(&fixture, replacement, frame_id(&resent)?).await?;
    assert_eq!(fixture.runtime.conformance_holder_observation()?.0, 0);
    assert!(matches!(
        old_socket.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    assert!(matches!(
        new_socket.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    fixture.objects.close().await?;
    Ok(())
}
