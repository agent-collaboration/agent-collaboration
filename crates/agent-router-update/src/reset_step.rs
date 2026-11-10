//! Step 0002: an enrollment reset an older Router recorded but didn't finish is finished
//! here, with the Host's own clearing rules, against the Fleet database under the name
//! that Router used, before any store moves or is renamed. A root without an unfinished
//! reset only records the step done.
use crate::{
    root_layout::{RootLayout, RouterUpdateError},
    update_barrier::{UpdateBarrier, wait_at},
};
use codex_router_host::{finish_enrollment_reset_for_update, router_collaboration_directory};
use message_board_storage::{RouterUpdateStep, UpdateLedger, UpdateStepProgress};
use std::path::{Path, PathBuf};

/// The journal an older Router's enrollment reset is recorded in.
pub(crate) fn enrollment_reset_journal(root: &Path) -> PathBuf {
    router_collaboration_directory(root).join("enrollment-reset.json")
}

pub(crate) async fn finish_enrollment_reset(root: &Path) -> Result<(), RouterUpdateError> {
    let layout = RootLayout::read(root).await?;
    finish_enrollment_reset_for_update(root, &layout.store)
        .await
        .map_err(|source| RouterUpdateError::File {
            path: enrollment_reset_journal(root),
            source,
        })?;
    let store_error = |source| RouterUpdateError::Store {
        store: layout.store.clone(),
        source,
    };
    let ledger = UpdateLedger::migrate_existing(&layout.store)
        .await
        .map_err(store_error)?;
    let recorded = ledger
        .record_step(
            RouterUpdateStep::FinishEnrollmentReset.number(),
            UpdateStepProgress::Done,
        )
        .await
        .map_err(store_error);
    ledger.close().await;
    recorded?;
    wait_at(UpdateBarrier::ResetDone)
        .await
        .map_err(RouterUpdateError::Barrier)
}
