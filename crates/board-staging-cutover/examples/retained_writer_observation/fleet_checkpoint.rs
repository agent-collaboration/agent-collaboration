//! Lead01a1c147-e4b9: public Fleet-cache observation on fresh staging scratch copies only.
use super::{ObservationError, check_sealed, seed_snapshot::check_scratch, source_hash};
use message_board_storage::FleetCache;
use std::{ffi::OsString, path::Path};

fn failure(error: impl std::fmt::Display) -> ObservationError {
    ObservationError::Read(error.to_string())
}

pub(super) async fn observe(
    scratch: &Path,
    untouched: &Path,
) -> Result<serde_json::Value, ObservationError> {
    check_sealed(untouched)?;
    check_scratch(scratch, untouched)?;
    // Native roots use fleet-cache.sqlite. Requiring the fresh cut's A/B names
    // prevents accidentally supplying that live native file to this opener.
    if scratch.file_name().is_none_or(|name| name != "A.sqlite")
        || untouched.file_name().is_none_or(|name| name != "B.sqlite")
        || scratch.parent() != untouched.parent()
    {
        return Err(ObservationError::InvalidInput);
    }
    let untouched_before = source_hash(untouched)?;
    let scratch_before = source_hash(scratch)?;
    let cache = FleetCache::open(scratch).await.map_err(failure)?;
    let result = cache.checkpoint().await.map_err(failure);
    cache.close().await;
    let checkpoint = result?;
    let untouched_after = source_hash(untouched)?;
    if untouched_before != untouched_after {
        return Err(ObservationError::SourceChanged);
    }
    Ok(serde_json::json!({
        "kind": "fleetCheckpointObserved",
        "checkpoint": checkpoint,
        "nonzero": checkpoint.get() > 0,
        "scratchBeforeSha256": scratch_before,
        "scratchAfterSha256": source_hash(scratch)?,
        "untouchedBeforeSha256": untouched_before,
        "untouchedAfterSha256": untouched_after,
        "liveRoutingOpened": false,
        "privateSqlUsed": false,
    }))
}

pub(super) async fn run(arguments: &[OsString]) -> Result<(), ObservationError> {
    let [_, scratch, untouched] = arguments else {
        return Err(ObservationError::InvalidInput);
    };
    let observed = observe(Path::new(scratch), Path::new(untouched)).await?;
    println!("{}", serde_json::to_string(&observed).map_err(failure)?);
    Ok(())
}

#[cfg(test)]
#[path = "fleet_checkpoint_test.rs"]
mod tests;
