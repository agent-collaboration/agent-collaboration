//! Exports a head-pinned public snapshot from disposable A; B is only hashed.
use super::{ObservationError, check_sealed, source_hash};
use board_fleet_sync::BoardProjectSchema;
use fleet_sync_lease::{LeaseHolder, LeasePolicy, WriterState};
use fleet_sync_protocol::{
    DatabaseId, Epoch, NoConsumer, PeerId, Piece, ReceiverId, ReplicationAnswer,
};
use fleet_sync_writer::{
    ReplicatedDatabase, ReplicationSender, ReplicationTransport, SendFailure, SenderOptions,
    WriterRegistrationOutcome,
};
use serde::Serialize;
use std::{ffi::OsString, os::unix::fs::MetadataExt, path::Path, sync::Arc, time::Duration};

pub(super) struct NoNetwork;
impl ReplicationTransport<NoConsumer> for NoNetwork {
    async fn send_piece(
        &self,
        _: ReceiverId,
        _: Piece<()>,
    ) -> Result<ReplicationAnswer, SendFailure> {
        Err(SendFailure::Unavailable {
            message: "snapshot extractor never sends pieces".to_owned(),
        })
    }
}

fn failure(error: impl std::fmt::Display) -> ObservationError {
    ObservationError::Read(error.to_string())
}
fn emit(value: &impl Serialize) -> Result<(), ObservationError> {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, value).map_err(failure)?;
    output.write_all(b"\n").map_err(failure)
}
fn identifier<T: serde::de::DeserializeOwned>(input: &OsString) -> Result<T, ObservationError> {
    let text = input.to_str().ok_or(ObservationError::InvalidInput)?;
    serde_json::from_value(serde_json::Value::String(text.to_owned())).map_err(failure)
}
pub(super) fn check_scratch(scratch: &Path, untouched: &Path) -> Result<(), ObservationError> {
    let metadata = std::fs::symlink_metadata(scratch).map_err(failure)?;
    let sealed = std::fs::symlink_metadata(untouched).map_err(failure)?;
    if !scratch.is_absolute()
        || std::fs::canonicalize(scratch).map_err(failure)? != scratch
        || !metadata.is_file()
        || metadata.mode() & 0o777 != 0o600
        || metadata.uid() != rustix::process::getuid().as_raw()
        || (metadata.dev(), metadata.ino()) == (sealed.dev(), sealed.ino())
        || scratch
            .components()
            .any(|part| part.as_os_str() == ".codex-router")
    {
        return Err(ObservationError::UnsealedFile);
    }
    Ok(())
}

pub(super) fn opening_holder(database: DatabaseId, peer: PeerId, lease: Epoch) -> LeaseHolder {
    // Retained seeded files can begin above genesis1. The same public context
    // is used by project_store_registry; SDK restoration applies the real receipt.
    let policy = LeasePolicy {
        lease_duration: Duration::from_secs(240),
        renew_interval: Duration::from_secs(60),
    };
    if lease.value() == 1 {
        LeaseHolder::provisional(database, peer, policy)
    } else {
        LeaseHolder::seeding(database, peer, lease, policy)
    }
}

pub(super) async fn run(arguments: &[OsString]) -> Result<(), ObservationError> {
    let [mode, scratch, untouched, database, peer, lease] = arguments else {
        return Err(ObservationError::InvalidInput);
    };
    let scratch = Path::new(scratch);
    let untouched = Path::new(untouched);
    check_sealed(untouched)?;
    check_scratch(scratch, untouched)?;
    let database: DatabaseId = identifier(database)?;
    let peer: PeerId = identifier(peer)?;
    let lease_text = lease.to_str().ok_or(ObservationError::InvalidInput)?;
    let lease_number = lease_text.parse::<u64>().map_err(failure)?;
    if lease_number.to_string() != lease_text {
        return Err(ObservationError::InvalidInput);
    }
    let lease = Epoch::new(lease_number).map_err(failure)?;
    let untouched_before = source_hash(untouched)?;
    let scratch_before = source_hash(scratch)?;
    let writer = Arc::new(
        ReplicatedDatabase::open(
            scratch,
            database,
            BoardProjectSchema::new().map_err(failure)?,
            opening_holder(database, peer, lease),
        )
        .await
        .map_err(failure)?,
    );
    let result = async {
        if mode == "--adopt-ledger-snapshot" {
            super::ledger_inspection::adopt(&writer).await?;
        }
        export(&writer, database, peer).await
    }
    .await;
    writer.close().await;
    let untouched_after = source_hash(untouched)?;
    if untouched_before != untouched_after {
        return Err(ObservationError::SourceChanged);
    }
    let exported = result?;
    emit(&serde_json::json!({
        "kind": "seedSnapshotComplete",
        "database": database,
        "holder": peer,
        "position": exported,
        "retainedFloor": null,
        "retainedFloorObserved": false,
        "scratchBeforeSha256": scratch_before,
        "scratchAfterSha256": source_hash(scratch)?,
        "untouchedBeforeSha256": untouched_before,
        "untouchedAfterSha256": untouched_after,
        "currentRenewalObserved": false
    }))
}

async fn export(
    writer: &Arc<ReplicatedDatabase<BoardProjectSchema>>,
    database: DatabaseId,
    peer: PeerId,
) -> Result<fleet_sync_protocol::ReplicationPosition, ObservationError> {
    let registration = writer.registration_outcome().await.map_err(failure)?;
    let WriterRegistrationOutcome::Ready { lease } = &registration else {
        return Err(ObservationError::RegistrationNotReady);
    };
    if lease.database != database || lease.holder != peer {
        return Err(ObservationError::LeaseMismatch);
    }
    let state = writer.writer_state().await.map_err(failure)?;
    if !matches!(
        state,
        WriterState::Writing { .. } | WriterState::Paused { .. }
    ) {
        return Err(ObservationError::OpeningState(format!("{state:?}")));
    }
    let sender = ReplicationSender::new(
        writer.clone(),
        Arc::new(()),
        Arc::new(()),
        Arc::new(NoNetwork),
        SenderOptions::default(),
    );
    // Local state read only. Never call add_copy, replicate_once or run.
    let copies = sender.copies().await.map_err(failure)?;
    let mut snapshot = writer.snapshot().await.map_err(failure)?;
    let position = snapshot.position();
    emit(&serde_json::json!({
        "kind": "seedSnapshotHeader", "database": database, "holder": peer,
        "registration": registration, "writerState": state,
        "position": position, "schema": snapshot.schema(), "copies": copies
    }))?;
    let manifest = snapshot.manifest().await.map_err(failure)?;
    let mut cursor = None;
    loop {
        let page = snapshot.page(cursor).await.map_err(failure)?;
        cursor = page.next.clone();
        emit(&serde_json::json!({ "kind": "seedSnapshotPage", "page": page }))?;
        if cursor.is_none() {
            break;
        }
    }
    emit(&serde_json::json!({ "kind": "seedSnapshotManifest", "manifest": manifest }))?;
    snapshot.close().await.map_err(failure)?;
    Ok(position)
}
