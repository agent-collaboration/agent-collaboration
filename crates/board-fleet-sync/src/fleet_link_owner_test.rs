use super::*;
use board_sync_protocol::{FleetLinkDropReason, FleetLinkRefusal};
use fleet_sync_protocol::{AuthRefusal, CloseReason, SessionId};
use fleet_sync_tokio_runner::{SessionConnectFailureKind, SessionDrop, SessionDropReason};
use runtime_primitives::SyncTime;
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

async fn prepared() -> TestSetup {
    let directory = tempfile::tempdir().unwrap();
    let cache = Arc::new(
        FleetCache::open(&directory.path().join("fleet-cache.sqlite"))
            .await
            .unwrap(),
    );
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (publisher, reader, owner) =
        FleetLinkOwner::prepare(cache.clone(), peer, "wss://central.example/session")
            .await
            .unwrap();
    (directory, cache, publisher, reader, owner)
}
type TestSetup = (
    tempfile::TempDir,
    Arc<FleetCache>,
    FleetLinkPublisher,
    FleetLinkReader,
    FleetLinkOwner,
);

fn time(second: i64) -> SyncTime {
    SyncTime::from_unix_millis(1_800_000_000_000 + second * 1_000)
}
fn connected(instance: u64, revision: u64) -> SessionLinkState {
    SessionLinkState {
        instance,
        revision,
        connection: 1,
        since: time(10),
        last_frame_at: Some(time(9)),
        status: SessionLinkStatus::Welcomed {
            session_id: SessionId::new(uuid::Uuid::now_v7()).unwrap(),
        },
        last_drop: None,
    }
}
fn disconnected(
    instance: u64,
    revision: u64,
    second: i64,
    reason: SessionDropReason,
) -> SessionLinkState {
    let status = match &reason {
        SessionDropReason::Disconnected { reason } => SessionLinkStatus::Disconnected {
            reason: reason.clone(),
        },
        _ => SessionLinkStatus::Disconnected { reason: None },
    };
    SessionLinkState {
        instance,
        revision,
        connection: 1,
        since: time(second),
        last_frame_at: Some(time(second)),
        status,
        last_drop: Some(SessionDrop {
            reason,
            at: time(second),
            connection: 1,
        }),
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Native observation and durable read assertions; fixture errors propagate."
)]
async fn native_final_drop_is_flushed_and_readable_after_owner_shutdown_and_reopen() -> TestResult {
    let (directory, cache, publisher, reader, owner) = prepared().await;
    let (native, observed) = watch::channel(connected(1, 1));
    publisher.attach_session(observed);
    let task = tokio::spawn(owner.run());
    let ready = reader.read().await?;
    assert!(
        matches!(ready.state, FleetLinkState::Connected { since, last_contact } if last_contact < since),
        "receipt before Welcome readiness must remain valid and unchanged"
    );
    native.send_replace(disconnected(1, 2, 20, SessionDropReason::IdleTimeout));
    drop(native);
    drop(publisher);
    task.await??;
    let record = cache.fleet_link().await?.ok_or("committed final link")?;
    assert_eq!(reader.read().await?, record);
    assert!(matches!(record.state, FleetLinkState::Disconnected { .. }));
    assert_eq!(
        record.last_drop.as_ref().ok_or("last drop")?.reason,
        FleetLinkDropReason::IdleTimeout
    );
    drop(reader);
    Arc::try_unwrap(cache)
        .map_err(|_| "cache still owned")?
        .close()
        .await;
    let reopened = FleetCache::open(&directory.path().join("fleet-cache.sqlite")).await?;
    assert_eq!(reopened.fleet_link().await?, Some(record));
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Instance/revision and barrier assertions; fixture errors propagate."
)]
async fn read_barrier_uses_latest_native_state_and_orders_revisions_only_within_one_instance()
-> TestResult {
    let (_directory, _cache, publisher, reader, owner) = prepared().await;
    let (native, observed) = watch::channel(SessionLinkState::default());
    publisher.attach_session(observed);
    // The owner has not been scheduled. Its first read must see this latest
    // native value without an independently scheduled forwarding task.
    native.send_replace(connected(1, 10));
    let task = tokio::spawn(owner.run());
    let current = reader.read().await?;
    assert!(matches!(current.state, FleetLinkState::Connected { .. }));
    native.send_replace(disconnected(1, 9, 30, SessionDropReason::IdleTimeout));
    assert_eq!(
        reader.read().await?,
        current,
        "lower revision replaced the accepted instance value"
    );
    native.send_replace(SessionLinkState {
        instance: 2,
        revision: 0,
        since: time(0),
        ..SessionLinkState::default()
    });
    assert!(
        matches!(reader.read().await?.state, FleetLinkState::Disconnected { since } if since.timestamp_millis() == time(0).unix_millis()),
        "new instance's revision zero was discarded"
    );
    native.send_replace(disconnected(
        2,
        1,
        40,
        SessionDropReason::Disconnected {
            reason: Some(CloseReason::ClientClosed),
        },
    ));
    drop(native);
    drop(publisher);
    task.await??;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Closed native observation must never fall back to a formerly connected record."
)]
async fn native_source_closing_without_a_terminal_snapshot_is_explicitly_unavailable() -> TestResult
{
    let (_directory, cache, publisher, reader, owner) = prepared().await;
    let (native, observed) = watch::channel(connected(1, 1));
    publisher.attach_session(observed);
    let task = tokio::spawn(owner.run());
    assert!(matches!(
        reader.read().await?.state,
        FleetLinkState::Connected { .. }
    ));
    drop(native);
    assert!(task.await?.is_err());
    assert!(
        reader.read().await.is_err(),
        "returned a stale connected fallback"
    );
    assert!(matches!(
        cache.fleet_link().await?.ok_or("prior receipt")?.state,
        FleetLinkState::Connected { .. }
    ));
    drop(publisher);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Typed native failure payloads must survive the actual owner and FleetCache."
)]
async fn native_failure_kinds_and_terminal_duplicate_refusal_are_persisted() -> TestResult {
    let (_directory, _cache, publisher, reader, owner) = prepared().await;
    let (native, observed) = watch::channel(SessionLinkState::default());
    publisher.attach_session(observed);
    let task = tokio::spawn(owner.run());
    let cases = [
        (
            SessionDropReason::ConnectFailed {
                kind: SessionConnectFailureKind::Unavailable,
            },
            FleetLinkFailureReason::Unavailable,
        ),
        (
            SessionDropReason::ConnectFailed {
                kind: SessionConnectFailureKind::InvalidEndpoint,
            },
            FleetLinkFailureReason::InvalidEndpoint,
        ),
        (
            SessionDropReason::ConnectFailed {
                kind: SessionConnectFailureKind::InvalidCredential,
            },
            FleetLinkFailureReason::InvalidCredential,
        ),
        (
            SessionDropReason::UnsupportedFrame,
            FleetLinkFailureReason::UnsupportedFrame,
        ),
        (
            SessionDropReason::SendTimeout,
            FleetLinkFailureReason::SendTimeout,
        ),
        (
            SessionDropReason::WelcomeTimeout,
            FleetLinkFailureReason::WelcomeTimeout,
        ),
        (
            SessionDropReason::CredentialRejected {
                reason: AuthRefusal::Expired,
            },
            FleetLinkFailureReason::CredentialRejected {
                reason: AuthRefusal::Expired,
            },
        ),
        (
            SessionDropReason::Revoked,
            FleetLinkFailureReason::CredentialRejected {
                reason: AuthRefusal::Revoked,
            },
        ),
        (
            SessionDropReason::ReplacedByNewerSession,
            FleetLinkFailureReason::ReplacedByNewerSession,
        ),
    ];
    for (index, (reason, expected)) in cases.into_iter().enumerate() {
        let revision = u64::try_from(index + 1)?;
        let mut snapshot = disconnected(1, revision, i64::try_from(index + 1)?, reason.clone());
        snapshot.status = SessionLinkStatus::Refused { reason };
        native.send_replace(snapshot);
        let record = reader.read().await?;
        assert_eq!(
            record.last_failure.ok_or("native failure")?.reason,
            expected
        );
        if expected == FleetLinkFailureReason::ReplacedByNewerSession {
            assert!(matches!(
                record.state,
                FleetLinkState::Refused {
                    reason: FleetLinkRefusal::IdentityInUseElsewhere,
                    ..
                }
            ));
        }
    }
    drop(native);
    drop(publisher);
    task.await??;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Returned native errors preserve logical status and expire only after a new native drop."
)]
async fn returned_native_failure_is_correlated_with_the_observed_drop_without_rewriting_state()
-> TestResult {
    let (_directory, _cache, publisher, reader, owner) = prepared().await;
    let first = disconnected(1, 1, 20, SessionDropReason::IdleTimeout);
    let (native, observed) = watch::channel(first.clone());
    publisher.attach_session(observed);
    let task = tokio::spawn(owner.run());
    let initial = reader.read().await?;
    publisher.session_failed(&ClientError::Store(StoreError::Corrupt {
        field: "machineSessionState".to_owned(),
    }));
    let failed = reader.read().await?;
    assert_eq!(failed.state, initial.state);
    assert_eq!(failed.last_drop, initial.last_drop);
    assert!(
        matches!(failed.last_failure.ok_or("returned failure")?.reason,
        FleetLinkFailureReason::CorruptStore { ref field } if field == "machineSessionState")
    );
    let mut ready = connected(1, 2);
    ready.last_drop = first.last_drop;
    native.send_replace(ready);
    assert!(matches!(
        reader
            .read()
            .await?
            .last_failure
            .ok_or("retained failure")?
            .reason,
        FleetLinkFailureReason::CorruptStore { .. }
    ));
    native.send_replace(disconnected(1, 3, 30, SessionDropReason::SendTimeout));
    assert_eq!(
        reader
            .read()
            .await?
            .last_failure
            .ok_or("new native failure")?
            .reason,
        FleetLinkFailureReason::SendTimeout
    );
    drop(native);
    drop(publisher);
    task.await??;
    Ok(())
}
