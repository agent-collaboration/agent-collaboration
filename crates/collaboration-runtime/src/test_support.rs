//! Test-only hooks; the normal Host and CLI dependency graphs do not enable these.
use std::{
    ffi::OsStr,
    fs,
    io::{self, ErrorKind},
    path::Path,
};

pub use crate::scheduled_route_fixture;
pub use crate::scheduled_run_worker::ScheduledRunWorker;
pub use crate::wakeup_delivery_sender::WakeDeliverySender;

/// Removes a private store fixture directory after its runtime has stopped.
///
/// SQLite can remove `-wal` and `-shm` files between directory enumeration and unlinking.
pub fn remove_store_directory_tolerating_sqlite_sidecars(directory: &Path) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) if error.kind() == ErrorKind::NotFound && is_sqlite_sidecar(&path) => {
                continue;
            }
            Err(error) => return Err(error),
        };
        if !file_type.is_file() {
            return Err(io::Error::other(format!(
                "store fixture directory contains an unsupported entry {}",
                path.display()
            )));
        }
        remove_store_file_tolerating_sqlite_sidecar(&path)?;
    }
    fs::remove_dir(directory).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "remove store fixture directory {}: {error}",
                directory.display()
            ),
        )
    })
}

fn remove_store_file_tolerating_sqlite_sidecar(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound && is_sqlite_sidecar(path) => Ok(()),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!("remove store fixture file {}: {error}", path.display()),
        )),
    }
}

fn is_sqlite_sidecar(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.ends_with("-wal") || name.ends_with("-shm"))
}

/// Test-only wake draft conversion at the durable evaluation boundary.
pub fn build_wake_push_draft(
    wake: &agent_automation::WakeRecord<
        collaboration_protocol::SavedMessage,
        collaboration_protocol::WakeCreator,
    >,
    fire: &agent_automation::FirstFire,
) -> Result<collaboration_protocol::PushRecordDraft, automation_storage::StorageError> {
    crate::wakeup_delivery_sender::build_wake_push_draft(wake, fire)
}

/// Test-only native boundary inputs and observations.
pub mod native_message_dispatch {
    pub use crate::native_message_dispatch::{
        NativeMessageBody, NativeMessageOutcome, NativeMessageParams, NativeMessageRequest,
        dispatch_message,
    };
}

/// Test-only native boundary inputs and observations.
pub mod message_effect_state {
    pub use crate::message_effect_state::{
        MessageEffects, NativeFailureDisposition, NativeFailureKind, NativeMessageAttemptEvidence,
        NativeMessageFailure,
    };
}

/// Test-only native boundary inputs and observations.
pub mod native_recorded_policy {
    pub use crate::native_recorded_policy::{
        RecordedNativeRouteClassification, classify_recorded_native_route,
        prepare_classified_native_policy,
    };
}

/// Test-only native boundary inputs and observations.
pub mod codex_queue_reconciliation {
    pub use crate::codex_queue_reconciliation::reconcile;
}

/// Test-only native boundary inputs and observations.
pub mod scheduled_run_evidence_sink {
    pub use crate::scheduled_run_evidence_sink::StoredRunEvidenceSink;
}

pub use crate::codex_app_server_delivery_route::native_result::interpret_native_response;

#[cfg(test)]
mod cleanup_tests {
    use super::*;

    #[test]
    fn cleanup_tolerates_only_vanished_sqlite_sidecars() {
        let directory = tempfile::tempdir_in("/tmp").expect("short fixture parent");
        let sidecar_path = directory.path().join("session-registry.sqlite-wal");
        fs::write(&sidecar_path, b"sidecar").expect("create sidecar");
        fs::remove_file(&sidecar_path).expect("simulate vanished sidecar");

        remove_store_file_tolerating_sqlite_sidecar(&sidecar_path)
            .expect("missing SQLite sidecar is already cleaned");

        let store_path = directory.path().join("session-registry.sqlite");
        fs::write(&store_path, b"store").expect("create main store file");
        fs::remove_file(&store_path).expect("simulate vanished main store file");

        let error = remove_store_file_tolerating_sqlite_sidecar(&store_path)
            .expect_err("missing main store file must remain an error");
        assert_eq!(error.kind(), ErrorKind::NotFound);

        let store_directory = directory.path().join("runtime-store");
        fs::create_dir(&store_directory).expect("create runtime store directory");
        fs::write(store_directory.join("session-registry.sqlite"), b"store")
            .expect("create runtime store file");
        remove_store_directory_tolerating_sqlite_sidecars(&store_directory)
            .expect("remove regular store files from the fixture root");
        assert!(
            !store_directory.exists(),
            "fixture root remains after shared cleanup"
        );
    }
}
