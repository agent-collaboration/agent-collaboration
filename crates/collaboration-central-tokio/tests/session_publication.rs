#![cfg(feature = "conformance")]

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "session_publication/support.rs"]
mod support;
use collaboration_central_core::ObjectHost;
use collaboration_central_tokio::SocketCommand;
use fleet_sync_protocol::{CloseReason, SchemaSupport};
use fleet_sync_session::ConnectionRegistration;
use std::{future::Future, sync::atomic::Ordering, task::Poll, time::Duration};
use support::{Fixture, TestError};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn register_refuses_another_peer_before_dispatch_and_keeps_the_socket_usable()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let (binding, session_id, mut socket) = fixture.socket().await?;
    fixture.hello(binding, session_id).await?;
    fixture.runtime.on_alarm().await?;
    let Some(SocketCommand::Frame(welcome)) = socket.try_recv().ok() else {
        return Err("attached fixture has no Welcome".into());
    };
    let calls_before = fixture
        .io
        .calls
        .lock()
        .map_err(|_| "call mutex poisoned")?
        .len();
    let frame = serde_json::json!({
        "kind":"request", "value":{
            "frame":"019f0000-0000-7000-8000-000000000301",
            "body":{"kind":"lease","value":{"kind":"register","value":{
                "attempt":"019f0000-0000-7000-8000-000000000302",
                "database":"019f0000-0000-7000-8000-000000000303",
                "writer":"019f0000-0000-7000-8000-000000000304",
                "origin":{"kind":"empty"}
            }}}
        }
    });

    let refused = fixture
        .runtime
        .conformance_frame(binding, serde_json::to_vec(&frame)?)
        .await;
    let answer = socket.try_recv();
    let calls_after = fixture
        .io
        .calls
        .lock()
        .map_err(|_| "call mutex poisoned")?
        .len();
    fixture.hello(binding, session_id).await?;
    let repeated = socket.try_recv();
    fixture.objects.close().await?;

    assert!(
        refused.is_ok(),
        "Register did not produce a session-peer refusal: {refused:?}"
    );
    let Ok(SocketCommand::Frame(answer)) = answer else {
        return Err("Register refusal missing on the requesting socket".into());
    };
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&answer)?,
        serde_json::json!({
            "kind":"answer", "value":{
                "frame":"019f0000-0000-7000-8000-000000000301",
                "body":{"kind":"lease","value":{"kind":"register","value":{
                    "kind":"refused","value":{"kind":"notWriter","value":{"current":null}}
                }}}
            }
        })
    );
    assert_eq!(calls_before, calls_after, "refusal reached another object");
    assert!(matches!(repeated, Ok(SocketCommand::Frame(bytes)) if bytes == welcome));
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn hello_commits_attaching_before_engine_publication_then_welcomes_once()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let (binding, session_id, mut socket) = fixture.socket().await?;

    fixture.hello(binding, session_id).await?;

    assert!(
        matches!(fixture.host.registrations().await?.get(&binding.connection), Some(ConnectionRegistration::Attaching { session_id: actual, .. }) if *actual == session_id)
    );
    assert!(matches!(
        socket.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    assert!(
        !fixture
            .io
            .wakes
            .lock()
            .map_err(|_| "wake mutex poisoned")?
            .is_empty()
    );
    fixture.runtime.on_alarm().await?;
    assert_eq!(
        fixture.host.registrations().await?.get(&binding.connection),
        Some(&ConnectionRegistration::Attached {
            session_id,
            schema_support: SchemaSupport { max_version: 7 }
        })
    );
    let Some(SocketCommand::Frame(bytes)) = socket.recv().await else {
        return Err("Welcome missing".into());
    };
    let frame: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(
        frame,
        serde_json::json!({"kind":"welcome","value":{"sessionId":session_id,"databases":[]}})
    );
    fixture.runtime.on_alarm().await?;
    assert!(matches!(
        socket.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    assert_eq!(
        fixture
            .io
            .calls
            .lock()
            .map_err(|_| "call mutex poisoned")?
            .iter()
            .filter(|call| call.get("kind").and_then(serde_json::Value::as_str)
                == Some("fleet-sync.report-schema-support"))
            .count(),
        1
    );
    fixture.objects.close().await?;
    Ok(())
}

#[tokio::test]
async fn callback_rollback_leaks_neither_attached_registration_nor_welcome() -> Result<(), TestError>
{
    rollback_case(1).await
}
#[tokio::test]
async fn sql_rollback_leaks_neither_attached_registration_nor_welcome() -> Result<(), TestError> {
    rollback_case(2).await
}
async fn rollback_case(fault: u8) -> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let (binding, session_id, mut socket) = fixture.socket().await?;
    fixture.hello(binding, session_id).await?;
    fixture.host.store.rollback.store(fault, Ordering::SeqCst);

    let report = fixture.runtime.on_alarm().await?;
    assert_eq!(report.retried.len(), 1);
    assert!(report.failed.is_empty());

    assert!(fixture.host.store.injected.load(Ordering::SeqCst));
    assert_eq!(
        fixture
            .host
            .store
            .after_rollback
            .lock()
            .map_err(|_| "rollback observation poisoned")?
            .as_deref(),
        Some("0")
    );
    assert!(
        matches!(fixture.host.registrations().await?.get(&binding.connection), Some(ConnectionRegistration::Attaching { session_id: actual, .. }) if *actual == session_id)
    );
    assert_eq!(fixture.attached_receipts().await?, "0");
    assert!(matches!(
        socket.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn publication_failure_quarantines_only_the_failed_connection_after_commit()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let (failed, failed_session, mut failed_socket) = fixture.socket().await?;
    // One connection object belongs to one installation and its lifetime peer.
    // Keep another transport opening while the first publication fails, then
    // complete that surviving transport's actual admission.
    let (healthy, healthy_session, mut healthy_socket) = fixture.socket_for(failed.peer).await?;
    fixture.hello(failed, failed_session).await?;
    *fixture
        .host
        .probe
        .fail
        .lock()
        .map_err(|_| "probe poisoned")? = Some(failed.connection);

    let report = fixture.runtime.on_alarm().await?;

    assert_eq!(report.completed.len(), 1);
    assert!(report.failed.is_empty());
    assert_eq!(
        fixture.host.registrations().await?.get(&failed.connection),
        Some(&ConnectionRegistration::Closed)
    );
    assert_eq!(
        fixture.host.admission_binding(failed.connection).await,
        Err(collaboration_central_core::SocketFailure::Closed)
    );
    assert_eq!(
        fixture.host.admission_binding(healthy.connection).await?,
        healthy
    );
    assert!(matches!(
        failed_socket.try_recv(),
        Ok(SocketCommand::Close(CloseReason::ClientClosed))
    ));
    assert!(matches!(
        failed_socket.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    assert_eq!(
        fixture.host.registrations().await?.get(&healthy.connection),
        Some(&ConnectionRegistration::Opening)
    );
    fixture.hello(healthy, healthy_session).await?;
    let surviving = fixture.runtime.on_alarm().await?;
    assert_eq!(surviving.completed.len() + report.completed.len(), 2);
    assert!(surviving.failed.is_empty());
    assert!(matches!(
        fixture.host.registrations().await?.get(&healthy.connection),
        Some(ConnectionRegistration::Attached { .. })
    ));
    assert!(matches!(
        healthy_socket.try_recv(),
        Ok(SocketCommand::Frame(_))
    ));
    assert!(matches!(
        healthy_socket.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn local_section_excludes_a_second_snapshot_until_committed_publication()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let (binding, session_id, _socket) = fixture.socket().await?;
    fixture.hello(binding, session_id).await?;
    fixture.host.probe.hold.store(true, Ordering::SeqCst);
    let runtime = fixture.runtime.clone();
    let drive = tokio::spawn(async move { runtime.on_alarm().await });
    tokio::time::timeout(Duration::from_secs(5), fixture.host.probe.entered.acquire())
        .await??
        .forget();
    // The finalization has committed its actual journal receipt, but local
    // publication is causally held by the capability wrapper.
    assert_eq!(fixture.attached_receipts().await?, "1");
    let host = fixture.host.clone();
    let mut snapshot = Box::pin(
        fixture
            .host
            .with_local_section(async move { host.registrations().await }),
    );

    let blocked = std::future::poll_fn(|context| {
        Poll::Ready(matches!(snapshot.as_mut().poll(context), Poll::Pending))
    })
    .await;
    assert!(
        blocked,
        "a second snapshot entered between commit and publication"
    );
    fixture.host.probe.release.add_permits(1);
    assert!(matches!(
        snapshot.await??.get(&binding.connection),
        Some(ConnectionRegistration::Attached { .. })
    ));
    tokio::time::timeout(Duration::from_secs(5), drive).await???;
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn interruption_after_commit_does_not_undo_the_receipt_or_emit_welcome()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let (binding, session_id, mut socket) = fixture.socket().await?;
    fixture.hello(binding, session_id).await?;
    fixture.host.probe.hold.store(true, Ordering::SeqCst);
    let runtime = fixture.runtime.clone();
    let drive = tokio::spawn(async move { runtime.on_alarm().await });
    tokio::time::timeout(Duration::from_secs(5), fixture.host.probe.entered.acquire())
        .await??
        .forget();
    assert_eq!(fixture.attached_receipts().await?, "1");

    drive.abort();
    assert!(drive.await.is_err_and(|error| error.is_cancelled()));

    assert_eq!(fixture.attached_receipts().await?, "1");
    assert!(matches!(
        socket.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    // A native task abort retains this heap. It does not establish a DO heap
    // loss with surviving sockets, so no eviction recovery is inferred here.
    assert!(matches!(
        fixture.host.registrations().await?.get(&binding.connection),
        Some(ConnectionRegistration::Attaching { .. })
    ));
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn welcome_gather_releases_local_exclusion_before_the_network_wait() -> Result<(), TestError>
{
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let (binding, session_id, mut socket) = fixture.socket().await?;
    fixture.hello(binding, session_id).await?;
    fixture.io.gather.hold.store(true, Ordering::SeqCst);
    let runtime = fixture.runtime.clone();
    let drive = tokio::spawn(async move { runtime.on_alarm().await });
    tokio::time::timeout(Duration::from_secs(5), fixture.io.gather.entered.acquire())
        .await??
        .forget();
    let host = fixture.host.clone();

    let snapshot = tokio::time::timeout(
        Duration::from_secs(5),
        fixture
            .host
            .with_local_section(async move { host.registrations().await }),
    )
    .await???;

    assert!(matches!(
        snapshot.get(&binding.connection),
        Some(ConnectionRegistration::Attached { .. })
    ));
    assert!(matches!(
        socket.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    fixture.io.gather.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), drive).await???;
    assert!(matches!(socket.try_recv(), Ok(SocketCommand::Frame(_))));
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_reopened_native_host_starts_with_no_transport_registrations() -> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let (binding, session_id, socket) = fixture.socket().await?;
    fixture.hello(binding, session_id).await?;
    assert!(matches!(
        fixture.host.registrations().await?.get(&binding.connection),
        Some(ConnectionRegistration::Attaching { .. })
    ));
    fixture.objects.close().await?;
    drop(socket);
    drop(fixture);

    let reopened = Fixture::reopen(directory.path()).await?;

    assert!(reopened.host.registrations().await?.is_empty());
    let report = reopened.runtime.on_alarm().await?;
    assert_eq!(report.completed.len(), 1);
    assert!(reopened.host.registrations().await?.is_empty());
    reopened.objects.close().await?;
    Ok(())
}
