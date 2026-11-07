//! J0, first part: the update tool on a root the old release populated. It refuses
//! while the Host holds the root, `--dry-run` changes nothing, a kill inside step 0001
//! reruns to a complete ledger with every value kept, a never-connected root gets its
//! store, and a `fleet.sqlite` no update made stops it.
use crate::update_support::{
    UpdateResult, WORKSPACE_IN_DATABASE, WORKSPACE_ONLY_IN_LOG, cached_workspace_ids,
    populated_root, run_update, run_update_killed_at, snapshot,
};
use codex_router_host::{HostCoordinationPaths, HostInstanceLock, router_fleet_cache_database};
use message_board_storage::{
    StoredUpdateLedger, UpdateLedgerEntry, UpdateStepNumber, UpdateStepProgress, read_update_ledger,
};
use std::path::Path;

fn ledger_with_step_0001_done() -> StoredUpdateLedger {
    StoredUpdateLedger::Present(vec![UpdateLedgerEntry::Step {
        step: UpdateStepNumber::new(1),
        progress: UpdateStepProgress::Done,
    }])
}

fn root_argument(root: &Path) -> [&std::ffi::OsStr; 2] {
    ["--router-root".as_ref(), root.as_os_str()]
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn the_fixture_keeps_one_committed_page_only_in_the_write_ahead_log() -> UpdateResult {
    // Arrange: the database file alone, without its log, beside the full copy.
    let (_directory, root) = populated_root()?;
    let store = router_fleet_cache_database(&root);
    let without_log = tempfile::tempdir()?;
    let database_only = without_log.path().join("fleet-cache.sqlite");
    std::fs::copy(&store, &database_only)?;

    // Act
    let from_database_alone = cached_workspace_ids(&database_only).await?;
    let with_its_log = cached_workspace_ids(&store).await?;

    // Assert: the update must carry a row that exists only in the log.
    assert_eq!(from_database_alone, vec![WORKSPACE_IN_DATABASE]);
    assert_eq!(
        with_its_log,
        vec![WORKSPACE_IN_DATABASE, WORKSPACE_ONLY_IN_LOG]
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn the_update_refuses_while_the_host_holds_the_root_and_changes_nothing() -> UpdateResult {
    // Arrange: hold the Host's own singleton lock, as a running Host does.
    let (_directory, root) = populated_root()?;
    let host = HostInstanceLock::acquire(&HostCoordinationPaths::for_router_root(&root))?;
    let before = snapshot(&root)?;

    // Act
    let output = run_update(&root_argument(&root)).await?;
    drop(host);

    // Assert
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("Host is running"), "{stderr}");
    assert_eq!(snapshot(&root)?, before);
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_dry_run_lists_the_pending_step_and_changes_nothing() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let before = snapshot(&root)?;

    // Act
    let mut arguments = root_argument(&root).to_vec();
    arguments.push("--dry-run".as_ref());
    let output = run_update(&arguments).await?;

    // Assert
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let store = router_fleet_cache_database(&root);
    assert_eq!(
        stdout,
        format!(
            "pending 0001: add the update ledger to {}\ndry run: nothing was changed\n",
            store.display()
        )
    );
    assert_eq!(snapshot(&root)?, before, "a dry run must not touch a byte");
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_kill_inside_step_0001_reruns_to_a_complete_ledger_keeping_every_value() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let store = router_fleet_cache_database(&root);
    let files_before: Vec<_> = snapshot(&root)?.into_keys().collect();

    // Act: killed after the ledger migration committed, before 0001 recorded itself.
    run_update_killed_at(&root, "step-0001:migrated").await?;
    let after_kill = read_update_ledger(&store).await?;
    let rerun = run_update(&root_argument(&root)).await?;

    // Assert
    assert_eq!(after_kill, StoredUpdateLedger::Present(vec![]));
    assert_eq!(
        rerun.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&rerun.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&rerun.stdout),
        format!("done 0001: add the update ledger to {}\n", store.display())
    );
    assert_eq!(
        read_update_ledger(&store).await?,
        ledger_with_step_0001_done()
    );
    assert_eq!(
        cached_workspace_ids(&store).await?,
        vec![WORKSPACE_IN_DATABASE, WORKSPACE_ONLY_IN_LOG],
        "the row that was only in the log must survive the kill and the rerun"
    );
    let store_files = |files: &[std::path::PathBuf]| {
        files
            .iter()
            .filter(|file| {
                file.extension()
                    .is_some_and(|extension| extension == "sqlite")
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    let files_after: Vec<_> = snapshot(&root)?.into_keys().collect();
    assert_eq!(
        store_files(&files_after),
        store_files(&files_before),
        "no store was created"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_kill_after_step_0001_is_done_leaves_nothing_for_a_rerun() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let store = router_fleet_cache_database(&root);

    // Act
    run_update_killed_at(&root, "step-0001:done").await?;
    let rerun = run_update(&root_argument(&root)).await?;

    // Assert
    assert_eq!(
        rerun.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&rerun.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&rerun.stdout),
        format!("{} needs no update\n", root.display())
    );
    assert_eq!(
        read_update_ledger(&store).await?,
        ledger_with_step_0001_done()
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_root_that_never_connected_gets_its_fleet_store_with_the_ledger() -> UpdateResult {
    // Arrange: a root with no Fleet database at all.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    std::fs::create_dir(&root)?;
    let store = router_fleet_cache_database(&root);

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
        format!(
            "done 0001: create {} with the update ledger\n",
            store.display()
        )
    );
    assert_eq!(
        read_update_ledger(&store).await?,
        ledger_with_step_0001_done()
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_fleet_database_no_update_made_stops_the_update_naming_it() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let foreign = codex_router_host::router_fleet_database(&root);
    std::fs::write(&foreign, b"not made by agent-router-update")?;
    let before = snapshot(&root)?;

    // Act
    let output = run_update(&root_argument(&root)).await?;

    // Assert
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains(&foreign.display().to_string()), "{stderr}");
    assert_eq!(snapshot(&root)?, before);
    Ok(())
}
