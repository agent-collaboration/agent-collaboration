//! Working-tree-only sensitivity cases; do not commit this seeded case.
use super::*;
use fleet_sync_protocol::{CommitSequence, Epoch, LeaseState, PeerId, SyncTime};
use fleet_sync_writer::RetainedFileOriginKind;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn known_file_context_preserves_seeded_lease_two_during_public_restore_sequence() -> TestResult {
    use fleet_sync_protocol::{RenewAnswer, RenewResult};
    let (database, peer) = identities()?;
    let epoch = Epoch::new(2)?;
    let mut holder = seed_snapshot::opening_holder(database, peer, epoch);
    // Mirrors registration.restore -> persisted grant -> restore_ready through
    // public lease APIs. A provisional genesis1 opener fails this sequence.
    holder.begin_seeding()?;
    let WriterRegistrationOutcome::Ready { lease } = ready(database, peer, epoch) else {
        return Err("fixture must supply Ready".into());
    };
    holder.observe_renewal(
        RenewAnswer::Renewed {
            recorded: RenewResult {
                epoch,
                expires_at: lease.expires_at,
            },
            current: lease,
            transfer_in_progress: None,
            replayed: true,
        },
        SyncTime::from_unix_millis(0),
    )?;
    holder.complete_seed(epoch)?;
    assert!(
        matches!(holder.writer_state(SyncTime::from_unix_millis(0))?, fleet_sync_lease::WriterState::Writing { epoch: observed } if observed == epoch)
    );
    Ok(())
}
fn identities() -> Result<(DatabaseId, PeerId), Box<dyn std::error::Error>> {
    Ok((
        DatabaseId::new(durable_steps::Uuid::now_v7())?,
        PeerId::try_from(durable_steps::Uuid::now_v7().to_string())?,
    ))
}
fn origin(database: DatabaseId, epoch: Epoch) -> RetainedFileOrigin {
    RetainedFileOrigin {
        database,
        epoch,
        start: ReplicationPosition {
            epoch,
            sequence: CommitSequence::new(0),
        },
        kind: RetainedFileOriginKind::Created,
    }
}
fn ready(database: DatabaseId, holder: PeerId, epoch: Epoch) -> WriterRegistrationOutcome {
    WriterRegistrationOutcome::Ready {
        lease: LeaseStatus {
            database,
            epoch,
            holder,
            state: LeaseState::Held,
            expires_at: SyncTime::from_unix_millis(1_900_000_000_000),
            remaining_ms: 10_000,
        },
    }
}
#[test]
fn matching_original_lease_is_observed_without_current_renewal_credit() -> TestResult {
    let (database, holder) = identities()?;
    let epoch = Epoch::new(3)?;
    let observed = validate_original_lease(
        database,
        origin(database, epoch),
        &ready(database, holder, epoch),
        false,
    )?;
    assert_eq!(observed.database, database);
    assert_eq!(observed.holder, holder);
    assert_eq!(observed.epoch, epoch);
    Ok(())
}
#[test]
fn seeded_wrong_lease_four_fails_when_retained_origin_is_three() -> TestResult {
    let (database, holder) = identities()?;
    let observed = validate_original_lease(
        database,
        origin(database, Epoch::new(3)?),
        &ready(database, holder, Epoch::new(4)?),
        false,
    );
    assert!(matches!(observed, Err(ObservationError::LeaseMismatch)));
    Ok(())
}
#[test]
fn pending_wrong_project_and_known_loss_fail_closed() -> TestResult {
    let (database, holder) = identities()?;
    let epoch = Epoch::new(3)?;
    assert!(matches!(
        validate_original_lease(
            database,
            origin(database, epoch),
            &WriterRegistrationOutcome::Pending,
            false
        ),
        Err(ObservationError::RegistrationNotReady)
    ));
    let (other, _) = identities()?;
    assert!(matches!(
        validate_original_lease(
            other,
            origin(database, epoch),
            &ready(database, holder, epoch),
            false
        ),
        Err(ObservationError::ProjectMismatch)
    ));
    assert!(matches!(
        validate_original_lease(
            database,
            origin(database, epoch),
            &ready(database, holder, epoch),
            true
        ),
        Err(ObservationError::AuthorityLost)
    ));
    Ok(())
}

#[tokio::test]
async fn stock_sqlite_backup_of_sdk_debug_file_reopens_through_retained_api() -> TestResult {
    use fleet_sync_lease::LeasePolicy;
    use fleet_sync_writer::{ReplicatedDatabase, WriterOptions};
    use std::{os::unix::fs::PermissionsExt, process::Command, time::Duration};
    // Arrange: a real native Turso file, made by the public SDK, not SQL fixtures.
    let directory = tempfile::tempdir()?;
    let canonical_directory = std::fs::canonicalize(directory.path())?;
    let original = canonical_directory.join("debug.sqlite");
    let backup = canonical_directory.join("backup.sqlite");
    let (_, peer) = identities()?;
    let writer = ReplicatedDatabase::create(
        &original,
        BoardProjectSchema::new()?,
        peer,
        LeasePolicy {
            lease_duration: Duration::from_secs(240),
            renew_interval: Duration::from_secs(60),
        },
        WriterOptions::default(),
    )
    .await?;
    let expected = writer.database();
    writer.close().await;
    // Act: the exact stock SQLite read-only backup route, then the guarded API.
    let destination = backup.to_str().ok_or("non-UTF8 owned temporary path")?;
    let copied = Command::new("/usr/bin/sqlite3")
        .args(["-readonly", "-nofollow", "-batch", "-bail"])
        .arg(&original)
        .arg(format!(".backup '{}'", destination.replace('\'', "''")))
        .output()?;
    assert!(
        copied.status.success(),
        "{}",
        String::from_utf8_lossy(&copied.stderr)
    );
    let integrity = Command::new("/usr/bin/sqlite3")
        .args(["-readonly", "-nofollow", "-batch", "-bail"])
        .arg(format!("file:{destination}?immutable=1"))
        .arg("PRAGMA integrity_check;")
        .output()?;
    assert!(integrity.status.success());
    assert_eq!(String::from_utf8(integrity.stdout)?.trim(), "ok");
    std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o400))?;
    let before = source_hash(&backup)?;
    let retained = RetainedFile::open_read_only(&backup, BoardProjectSchema::new()?).await?;
    // Assert: same generated database and unchanged backup bytes. Pending means
    // this is format qualification only; no successful real registration claimed.
    assert_eq!(retained.origin().database, expected);
    assert!(matches!(
        retained.registration_outcome(),
        WriterRegistrationOutcome::Pending
    ));
    assert_eq!(source_hash(&backup)?, before);
    Ok(())
}

#[tokio::test]
async fn shared_genesis_fixture_runs_complete_public_snapshot_export() -> TestResult {
    use fleet_sync_lease::{LeaseHolder, LeasePolicy};
    use fleet_sync_protocol::{CopyRole, ReceiverId};
    use fleet_sync_writer::{ReplicatedDatabase, ReplicationSender, SenderOptions, WriterOptions};
    use std::{
        ffi::OsString, os::unix::fs::PermissionsExt, process::Command, sync::Arc, time::Duration,
    };
    // Fixed UUIDv7 fixture identities, no production rows, credentials or network.
    let database = DatabaseId::new(durable_steps::Uuid::parse_str(
        "01a1c0c3-0000-7000-8000-000000000001",
    )?)?;
    let peer = PeerId::try_from("01a1c0c3-0000-7000-8000-000000000002".to_owned())?;
    let receiver = ReceiverId::new(durable_steps::Uuid::parse_str(
        "01a1c0c3-0000-7000-8000-000000000003",
    )?)?;
    let directory = tempfile::tempdir()?;
    let root = std::fs::canonicalize(directory.path())?;
    let source = root.join("source.sqlite");
    let untouched = root.join("B.sqlite");
    let scratch = root.join("A.sqlite");
    let writer = Arc::new(
        ReplicatedDatabase::open_with(
            &source,
            database,
            BoardProjectSchema::new()?,
            LeaseHolder::provisional(
                database,
                peer,
                LeasePolicy {
                    lease_duration: Duration::from_secs(240),
                    renew_interval: Duration::from_secs(60),
                },
            ),
            Arc::new(runtime_primitives::SystemClock),
            WriterOptions::default(),
        )
        .await?,
    );
    writer
        .observe_registration(
            LeaseStatus {
                database,
                holder: peer,
                epoch: Epoch::new(1)?,
                state: LeaseState::Held,
                expires_at: SyncTime::from_unix_millis(600_000),
                remaining_ms: 600_000,
            },
            SyncTime::from_unix_millis(0),
        )
        .await?;
    let mut snapshot = writer.snapshot().await?;
    let head = snapshot.position();
    let manifest = snapshot.manifest().await?;
    snapshot.close().await?;
    let sender = ReplicationSender::new(
        writer.clone(),
        Arc::new(()),
        Arc::new(()),
        Arc::new(seed_snapshot::NoNetwork),
        SenderOptions::default(),
    );
    sender
        .add_copy(receiver, CopyRole::Authoritative, head)
        .await?;
    drop(sender);
    // Exercise the real public writer-journal initialization before taking the
    // synthetic cut; the private SQL inspector must decode SQLx BOOLEAN receipts.
    ledger_inspection::adopt(&writer).await?;
    writer.close().await;
    let backup = Command::new("/usr/bin/sqlite3")
        .args(["-readonly", "-nofollow", "-batch", "-bail"])
        .arg(&source)
        .arg(format!(".backup '{}'", untouched.display()))
        .output()?;
    assert!(
        backup.status.success(),
        "{}",
        String::from_utf8_lossy(&backup.stderr)
    );
    std::fs::set_permissions(&untouched, std::fs::Permissions::from_mode(0o400))?;
    let duplicated = Command::new("/usr/bin/sqlite3")
        .args(["-readonly", "-nofollow", "-batch", "-bail"])
        .arg(format!("file:{}?immutable=1", untouched.display()))
        .arg(format!(".backup '{}'", scratch.display()))
        .output()?;
    assert!(
        duplicated.status.success(),
        "{}",
        String::from_utf8_lossy(&duplicated.stderr)
    );
    std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o600))?;
    let before = source_hash(&untouched)?;
    ledger_inspection::run(&[
        OsString::from("--inspect-ledger"),
        scratch.clone().into_os_string(),
        untouched.clone().into_os_string(),
    ])
    .await?;
    // Act: the same A/B seed-export entry point, including real SDK reopening,
    // registered receiver read, complete pages and final B hash check.
    seed_snapshot::run(&[
        OsString::from("--seed-snapshot"),
        scratch.into_os_string(),
        untouched.clone().into_os_string(),
        OsString::from(database.as_uuid().to_string()),
        OsString::from(peer.as_uuid().to_string()),
        OsString::from("1"),
    ])
    .await?;
    assert_eq!(source_hash(&untouched)?, before);
    // Stable SDK-owned comparison surface across independently signed Mac builds.
    println!(
        "sharedFixtureManifest={}",
        serde_json::to_string(&manifest)?
    );
    Ok(())
}
