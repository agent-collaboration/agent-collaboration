//! S8 on Tokio: the object's durable state, fenced by the socket table.
//!
//! Served from the slot without building anything: an object with no host or
//! kernel in this process has no open store, so it answers `notOpen`. The
//! socket table is read before and after the durable transaction; if it
//! changed, the answer is `snapshotUnstable` and the caller retries.

use super::{ObjectSlot, RuntimeFace};
use collaboration_central_conformance::{RuntimeControlAnswer, RuntimeControlFailure};
use collaboration_central_core::conformance_snapshot::{SnapshotRefusal, fenced};

pub(super) async fn snapshot(
    slot: &ObjectSlot,
) -> Result<RuntimeControlAnswer, RuntimeControlFailure> {
    let (Some(host), Some(runtime)) = (slot.host.get(), slot.current_runtime()) else {
        return Err(RuntimeControlFailure::NotOpen);
    };
    let before = host.registration_rows().await;
    let durable = match runtime.as_ref() {
        RuntimeFace::Fleet(kernel) => kernel.face().conformance_snapshot_durable().await,
        RuntimeFace::Copy(kernel) => kernel.face().conformance_snapshot_durable().await,
    }
    .map_err(refused)?;
    let after = host.registration_rows().await;
    fenced(before, durable, &after)
        .map(RuntimeControlAnswer::Snapshot)
        .map_err(refused)
}

fn refused(refusal: SnapshotRefusal) -> RuntimeControlFailure {
    eprintln!("S8 snapshot refused: {refusal}");
    match refusal {
        SnapshotRefusal::CorruptRow { table, key, field } => RuntimeControlFailure::CorruptRow {
            table: table.to_owned(),
            key,
            field: field.to_owned(),
        },
        SnapshotRefusal::Storage(_) => RuntimeControlFailure::StorageFailed,
        SnapshotRefusal::Unstable => RuntimeControlFailure::SnapshotUnstable,
    }
}

#[cfg(test)]
#[path = "snapshot_control_test.rs"]
mod tests;
