//! J4 (moves): steps 0005 and 0010 on the populated root. Every value the old release
//! wrote ends up in `fleet.sqlite` exactly, including rows only in write-ahead logs,
//! through a kill at every barrier; the older stores are gone afterwards; a merged row
//! is still log-only right before the checkpoint that must save it; and a malformed
//! source stops step 0005 before it changes anything.
use crate::update_ledger_journey::{completed_ledger, full_update_report, root_argument};
use crate::update_support::{
    CREATION_REQUEST, CURSOR_KEY_BYTES, DECIDED_AT_MS, DECIDED_REQUEST, FleetValues,
    PENDING_REQUEST, UpdateResult, WORKSPACE_IN_DATABASE, WORKSPACE_ONLY_IN_LOG, populated_root,
    run_update, run_update_inspecting_at, run_update_killed_at, snapshot,
};
use codex_router_host::{
    RouterUpdateRequirement, inspect_router_update_requirement, router_collaboration_directory,
    router_fleet_cache_database, router_fleet_database,
};
use message_board_storage::{
    StoredUpdateLedger, UpdateLedgerEntry, UpdateStepNumber, UpdateStepProgress, read_update_ledger,
};
use std::path::Path;

const OLDER_STORE_FILES: [&str; 6] = [
    "fleet-cache.sqlite",
    "fleet-cache.sqlite-wal",
    "fleet-outbox.sqlite",
    "fleet-outbox.sqlite-wal",
    "board-cursors.sqlite",
    "board-cursors.sqlite-wal",
];

/// The fixture's values, checked against the builder's literals before any update.
#[expect(
    clippy::panic_in_result_fn,
    reason = "Fixture literal checks fail the test; read errors propagate through Result."
)]
async fn fixture_values(root: &Path) -> UpdateResult<FleetValues> {
    let values = FleetValues::read_legacy(root).await?;
    let request_ids: Vec<&str> = values.requests.iter().map(|row| row.0.as_str()).collect();
    assert_eq!(
        request_ids,
        [PENDING_REQUEST, DECIDED_REQUEST, CREATION_REQUEST]
    );
    let decided = values
        .requests
        .iter()
        .find(|row| row.0 == DECIDED_REQUEST)
        .ok_or("decided request")?;
    assert_eq!(decided.4, Some(DECIDED_AT_MS));
    assert_eq!(values.creation_inputs.len(), 1);
    assert_eq!(values.creation_inputs[0].0, CREATION_REQUEST);
    assert_eq!(values.cursor_keys.len(), 1);
    assert_eq!(values.cursor_keys[0].1, CURSOR_KEY_BYTES);
    let workspaces: Vec<&str> = values
        .cached_records
        .iter()
        .map(|row| row.1.as_str())
        .collect();
    assert_eq!(workspaces, [WORKSPACE_IN_DATABASE, WORKSPACE_ONLY_IN_LOG]);
    assert_eq!(values.checkpoint, 2);
    Ok(values)
}

/// After a finished update: the values, the ledger, the guard and the root listing.
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions fail the test; read errors propagate through Result."
)]
async fn assert_update_finished(
    root: &Path,
    expected: &FleetValues,
    context: &str,
) -> UpdateResult {
    let current = router_fleet_database(root);
    assert_eq!(
        &FleetValues::read_merged(&current).await?,
        expected,
        "{context}: every value must be in fleet.sqlite exactly"
    );
    assert_eq!(
        read_update_ledger(&current).await?,
        completed_ledger(true),
        "{context}: ledger"
    );
    assert_eq!(
        inspect_router_update_requirement(root).await?,
        RouterUpdateRequirement::NotRequired,
        "{context}: guard"
    );
    let directory = router_collaboration_directory(root);
    for file in OLDER_STORE_FILES {
        assert!(
            !directory.join(file).exists(),
            "{context}: {file} must be gone"
        );
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_full_update_carries_every_value_and_removes_the_older_stores() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let expected = fixture_values(&root).await?;

    // Act
    let output = run_update(&root_argument(&root)).await?;

    // Assert
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        full_update_report(&root, "done")
    );
    assert_update_finished(&root, &expected, "single run").await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_kill_at_any_move_barrier_reruns_to_the_same_values() -> UpdateResult {
    let markers = [
        "step-0005:fleet-outbox.sqlite:written",
        "step-0005:fleet-outbox.sqlite:verified",
        "step-0005:fleet-outbox.sqlite:database-removed",
        "step-0005:fleet-outbox.sqlite:removed",
        "step-0005:board-cursors.sqlite:written",
        "step-0005:board-cursors.sqlite:verified",
        "step-0005:board-cursors.sqlite:database-removed",
        "step-0005:board-cursors.sqlite:removed",
        "step-0005:done",
        "step-0010:before-checkpoint",
        "step-0010:checkpointed",
        "step-0010:renamed",
        "step-0010:done",
    ];
    for marker in markers {
        // Arrange
        let (_directory, root) = populated_root()?;
        let expected = fixture_values(&root).await?;

        // Act
        run_update_killed_at(&root, marker).await?;
        let refused = inspect_router_update_requirement(&root).await?;
        let rerun = run_update(&root_argument(&root)).await?;

        // Assert: until the last ledger write the Host must not start; after it, the
        // update is complete and it may.
        let update_complete = marker == "step-0010:done";
        assert_eq!(
            refused == RouterUpdateRequirement::NotRequired,
            update_complete,
            "killed at {marker}: guard said {refused:?}"
        );
        assert_eq!(
            rerun.status.code(),
            Some(0),
            "killed at {marker}: {}",
            String::from_utf8_lossy(&rerun.stderr)
        );
        assert_update_finished(&root, &expected, &format!("killed at {marker}")).await?;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn merged_rows_are_still_log_only_right_before_the_rename_checkpoint() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let expected = fixture_values(&root).await?;
    let former = router_fleet_cache_database(&root);
    let copies = tempfile::tempdir()?;

    // Act: hold the update just before its checkpoint and look at the file alone.
    let status = run_update_inspecting_at(&root, "step-0010:before-checkpoint", || async {
        let database_only = copies.path().join("database-only.sqlite");
        std::fs::copy(&former, &database_only)?;
        let with_log = FleetValues::read_merged(&former).await?;
        assert!(
            with_log.requests.iter().any(|row| row.0 == DECIDED_REQUEST),
            "the merged decided request is readable with its log"
        );
        let without_log = FleetValues::read_merged(&database_only).await;
        let decided_in_file = without_log
            .as_ref()
            .is_ok_and(|values| values.requests.iter().any(|row| row.0 == DECIDED_REQUEST));
        assert!(
            !decided_in_file,
            "the merged decided request must still be only in the log here, so the checkpoint is what saves it"
        );
        Ok(())
    })
    .await?;

    // Assert
    assert!(
        status.success(),
        "the held update must finish once released"
    );
    assert_update_finished(&root, &expected, "held before checkpoint").await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_malformed_source_stops_the_merge_before_it_changes_anything() -> UpdateResult {
    // Arrange: the outbox store is not a database at all.
    let (_directory, root) = populated_root()?;
    let directory = router_collaboration_directory(&root);
    let outbox = directory.join("fleet-outbox.sqlite");
    let outbox_log = directory.join("fleet-outbox.sqlite-wal");
    let original_outbox = (std::fs::read(&outbox)?, std::fs::read(&outbox_log)?);
    let expected = fixture_values(&root).await?;
    std::fs::write(&outbox, b"this is not a database")?;
    std::fs::remove_file(&outbox_log)?;
    let sources_before = snapshot(&directory)?;

    // Act
    let refused = run_update(&root_argument(&root)).await?;

    // Assert: 0001 stays done; nothing of 0005 happened; nothing was renamed.
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains(&outbox.display().to_string()), "{stderr}");
    let former = router_fleet_cache_database(&root);
    assert_eq!(
        read_update_ledger(&former).await?,
        StoredUpdateLedger::Present(vec![UpdateLedgerEntry::Step {
            step: UpdateStepNumber::new(1),
            progress: UpdateStepProgress::Done,
        }])
    );
    let merged = FleetValues::read_merged(&former).await?;
    assert!(merged.requests.is_empty() && merged.cursor_keys.is_empty());
    let sources_after = snapshot(&directory)?;
    for source in [
        "fleet-outbox.sqlite",
        "board-cursors.sqlite",
        "board-cursors.sqlite-wal",
    ] {
        assert_eq!(
            sources_after.get(Path::new(source)),
            sources_before.get(Path::new(source)),
            "{source} must be untouched"
        );
    }
    assert!(
        !router_fleet_database(&root).exists(),
        "nothing was renamed"
    );

    // Act and assert again: once the source is repaired, the update completes.
    std::fs::write(&outbox, &original_outbox.0)?;
    std::fs::write(&outbox_log, &original_outbox.1)?;
    let repaired = run_update(&root_argument(&root)).await?;
    assert_eq!(
        repaired.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&repaired.stderr)
    );
    assert_update_finished(&root, &expected, "after repair").await
}
