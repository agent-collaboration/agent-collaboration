//! Verification-only observations: passive retained read or an explicit scratch A writer.
use board_fleet_sync::BoardProjectSchema;
use fleet_sync_protocol::{DatabaseId, LeaseStatus, ReplicationPosition};
use fleet_sync_writer::{RetainedFile, RetainedFileOrigin, WriterRegistrationOutcome};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path, process::ExitCode};

mod fleet_checkpoint;
mod ledger_inspection;
mod renewal_observation;
mod seed_snapshot;

#[derive(Debug, thiserror::Error)]
enum ObservationError {
    #[error("input must be one explicit canonical sealed backup and an expected project UUID")]
    InvalidInput,
    #[error("sealed backup is not an owner-private read-only regular file")]
    UnsealedFile,
    #[error("retained origin differs from the expected project")]
    ProjectMismatch,
    #[error("registration is pending, refused or failed")]
    RegistrationNotReady,
    #[error("registration database or lease does not match the retained origin")]
    LeaseMismatch,
    #[error("retained file records loss of writer authority")]
    AuthorityLost,
    #[error("local opening context is not restored to Writing/Paused: {0}")]
    OpeningState(String),
    #[error("sealed source bytes changed during observation")]
    SourceChanged,
    #[error("retained read failed: {0}")]
    Read(String),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WriterObservation {
    kind: &'static str,
    database: DatabaseId,
    holder: fleet_sync_protocol::PeerId,
    lease: fleet_sync_protocol::Epoch,
    origin_epoch: fleet_sync_protocol::Epoch,
    origin_start: ReplicationPosition,
    original_registration: LeaseStatus,
    retained_authority: &'static str,
    source_sha256: String,
    current_renewal_observed: bool,
}
fn validate_original_lease(
    expected: DatabaseId,
    origin: RetainedFileOrigin,
    registration: &WriterRegistrationOutcome,
    authority_lost: bool,
) -> Result<LeaseStatus, ObservationError> {
    if origin.database != expected {
        return Err(ObservationError::ProjectMismatch);
    }
    if authority_lost {
        return Err(ObservationError::AuthorityLost);
    }
    let WriterRegistrationOutcome::Ready { lease } = registration else {
        return Err(ObservationError::RegistrationNotReady);
    };
    if lease.database != expected || lease.epoch != origin.epoch {
        return Err(ObservationError::LeaseMismatch);
    }
    Ok(lease.clone())
}
fn source_hash(path: &Path) -> Result<String, ObservationError> {
    let mut source =
        std::fs::File::open(path).map_err(|error| ObservationError::Read(error.to_string()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = source
            .read(&mut buffer)
            .map_err(|error| ObservationError::Read(error.to_string()))?;
        if count == 0 {
            break;
        }
        hash.update(
            buffer
                .get(..count)
                .ok_or_else(|| ObservationError::Read("invalid read length".to_owned()))?,
        );
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn check_sealed(path: &Path) -> Result<(), ObservationError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| ObservationError::Read(error.to_string()))?;
    if !path.is_absolute()
        || std::fs::canonicalize(path).map_err(|error| ObservationError::Read(error.to_string()))?
            != path
        || !metadata.is_file()
        || metadata.mode() & 0o222 != 0
        || metadata.mode() & 0o077 != 0
        || metadata.uid() != rustix::process::getuid().as_raw()
        || path
            .components()
            .any(|part| part.as_os_str() == ".codex-router")
    {
        return Err(ObservationError::UnsealedFile);
    }
    Ok(())
}
async fn observe(path: &Path, expected: DatabaseId) -> Result<WriterObservation, ObservationError> {
    check_sealed(path)?;
    let before = source_hash(path)?;
    let retained = RetainedFile::open_read_only(
        path,
        BoardProjectSchema::new().map_err(|error| ObservationError::Read(error.to_string()))?,
    )
    .await
    .map_err(|error| ObservationError::Read(error.to_string()))?;
    let origin = retained.origin();
    let lease = validate_original_lease(
        expected,
        origin,
        retained.registration_outcome(),
        retained.authority().is_some(),
    )?;
    let after = source_hash(path)?;
    if before != after {
        return Err(ObservationError::SourceChanged);
    }
    Ok(WriterObservation {
        kind: "retainedWriterObserved",
        database: lease.database,
        holder: lease.holder,
        lease: lease.epoch,
        origin_epoch: origin.epoch,
        origin_start: origin.start,
        original_registration: lease,
        retained_authority: "noObservedLoss",
        source_sha256: before,
        current_renewal_observed: false,
    })
}
#[tokio::main]
async fn main() -> ExitCode {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_some_and(|argument| argument == "--renewal-observation")
    {
        return match renewal_observation::run(&arguments).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("scratch renewal observation incomplete: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if arguments
        .first()
        .is_some_and(|argument| argument == "--fleet-checkpoint")
    {
        return match fleet_checkpoint::run(&arguments).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("scratch Fleet checkpoint refused: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if arguments.len() == 1
        && arguments
            .first()
            .is_some_and(|argument| argument == "--help")
    {
        println!(
            "retained_writer_observation <absolute-mode0400-sealed-backup> <expected-project-uuid>\nretained_writer_observation --seed-snapshot <scratch-A-mode0600> <untouched-B-mode0400> <expected-project-uuid> <expected-peer-uuid> <original-lease-number>\nretained_writer_observation --adopt-ledger-snapshot <scratch-A-mode0600> <untouched-B-mode0400> <expected-project-uuid> <expected-peer-uuid> <original-lease-number>\nretained_writer_observation --inspect-ledger <scratch-A-mode0600> <untouched-B-mode0400>\nretained_writer_observation --tamper-ledger-checksum <sacrificial-A-mode0600> <untouched-B-mode0400>\nOrdinary mode is passive. Seed mode opens only disposable A writable through the public SDK; B is hashed only. Seed output is JSON Lines and is incomplete unless the final seedSnapshotComplete record exists. Neither normal mode proves current renewal or uses private SQL/network. Ledger modes use only the separately authorized scratch-only ledger/window SQL; tamper mode performs exactly one checksum UPDATE."
        );
        println!(
            "retained_writer_observation --fleet-checkpoint <fresh-routing-A.sqlite-mode0600> <sealed-routing-B.sqlite-mode0400>\nFleet mode uses public FleetStore::open_existing and its cache checkpoint only on A; B is hashed only. It starts no Router or network and does not open live/production routing files."
        );
        println!(
            "retained_writer_observation --renewal-observation <fresh-project-A.sqlite-mode0600> <sealed-project-B.sqlite-mode0400> <D> <M> <N> <window-start-unix-ms> <window-end-unix-ms>\nReads exactly the separately granted scratch-A authority projection; requires a public RenewAnswer::Renewed at same D/M/N within the observation window. No live/B SDK open or SQL write."
        );
        return ExitCode::SUCCESS;
    }
    if arguments.first().is_some_and(|argument| {
        argument == "--seed-snapshot" || argument == "--adopt-ledger-snapshot"
    }) {
        return match seed_snapshot::run(&arguments).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("scratch seed snapshot refused: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if arguments.first().is_some_and(|argument| {
        argument == "--inspect-ledger" || argument == "--tamper-ledger-checksum"
    }) {
        return match ledger_inspection::run(&arguments).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("scratch ledger observation refused: {error}");
                ExitCode::FAILURE
            }
        };
    }
    let result = async {
        let [path, project] = arguments.as_slice() else {
            return Err(ObservationError::InvalidInput);
        };
        let project = project.to_str().ok_or(ObservationError::InvalidInput)?;
        let expected =
            serde_json::from_value::<DatabaseId>(serde_json::Value::String(project.to_owned()))
                .map_err(|_| ObservationError::InvalidInput)?;
        observe(Path::new(path), expected).await
    }
    .await;
    match result {
        Ok(observed) => match serde_json::to_string(&observed) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("serialization failed: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("retained writer observation refused: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
#[path = "retained_writer_observation_test.rs"]
mod tests;
