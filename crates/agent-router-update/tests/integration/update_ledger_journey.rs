//! J0: the update tool on a root the old release populated. It refuses while the Host
//! holds the root, `--dry-run` changes nothing, a kill inside step 0001 reruns to a
//! complete update with every value kept, a never-connected root ends with its Fleet
//! database, a `fleet.sqlite` no update made stops it, and a store a Host left
//! unrecorded is refused until the tool completes it.
use crate::update_support::{
    FleetValues, UpdateResult, WORKSPACE_IN_DATABASE, WORKSPACE_ONLY_IN_LOG, cached_workspace_ids,
    populated_root, run_update, run_update_killed_at, snapshot,
};
use codex_router_host::{
    HostCoordinationPaths, HostInstanceLock, RequiredRouterUpdate, RouterUpdateRequirement,
    inspect_router_update_requirement, router_collaboration_directory, router_fleet_cache_database,
    router_fleet_database,
};
use message_board_storage::{
    RouterUpdateStep, StoredUpdateLedger, UpdateLedger, UpdateLedgerEntry, UpdateSourceProgress,
    UpdateStepNumber, UpdateStepProgress, read_update_ledger,
};
use std::path::Path;

/// The ledger a completed update leaves: every step done, each merged source removed.
pub fn completed_ledger(with_sources: bool) -> StoredUpdateLedger {
    let mut entries = Vec::new();
    for step in RouterUpdateStep::KNOWN {
        entries.push(UpdateLedgerEntry::Step {
            step: step.number(),
            progress: UpdateStepProgress::Done,
        });
        if with_sources && step == RouterUpdateStep::MergeFleetStores {
            // Ledger rows read back in (step, source) order.
            for source in ["board-cursors.sqlite", "fleet-outbox.sqlite"] {
                entries.push(UpdateLedgerEntry::Source {
                    step: step.number(),
                    source: source.to_owned(),
                    progress: UpdateSourceProgress::Removed,
                });
            }
        }
    }
    StoredUpdateLedger::Present(entries)
}

fn unfinished(store: &Path, steps: &[u16]) -> RouterUpdateRequirement {
    RouterUpdateRequirement::Required(RequiredRouterUpdate::UnfinishedSteps {
        store: store.to_owned(),
        steps: steps.iter().copied().map(UpdateStepNumber::new).collect(),
    })
}

pub fn root_argument(root: &Path) -> [&std::ffi::OsStr; 2] {
    ["--router-root".as_ref(), root.as_os_str()]
}

/// What a full update of the populated fixture reports, step by step.
pub fn full_update_report(root: &Path, verb: &str) -> String {
    let directory = router_collaboration_directory(root);
    let former = router_fleet_cache_database(root);
    format!(
        "{verb} 0001: add the update ledger to {former}\n\
         {verb} 0005: move {outbox} and {cursors} into {former}, then remove them\n\
         {verb} 0010: rename {former} to {current}\n",
        former = former.display(),
        outbox = directory.join("fleet-outbox.sqlite").display(),
        cursors = directory.join("board-cursors.sqlite").display(),
        current = router_fleet_database(root).display(),
    )
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
async fn a_dry_run_lists_every_pending_step_and_changes_nothing() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let before = snapshot(&root)?;

    // Act
    let mut arguments = root_argument(&root).to_vec();
    arguments.push("--dry-run".as_ref());
    let output = run_update(&arguments).await?;

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
            "{}dry run: nothing was changed\n",
            full_update_report(&root, "pending")
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
async fn a_kill_inside_step_0001_reruns_to_a_complete_update_keeping_every_value() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let former = router_fleet_cache_database(&root);
    let fixture_values = FleetValues::read_legacy(&root).await?;

    // Act: killed after the ledger migration committed, before 0001 recorded itself.
    run_update_killed_at(&root, "step-0001:migrated").await?;
    let after_kill = read_update_ledger(&former).await?;
    let guard_after_kill = inspect_router_update_requirement(&root).await?;
    let rerun = run_update(&root_argument(&root)).await?;

    // Assert
    assert_eq!(after_kill, StoredUpdateLedger::Present(vec![]));
    assert_eq!(
        guard_after_kill,
        unfinished(&former, &[1, 5, 10]),
        "a Host must not start on the interrupted update"
    );
    assert_eq!(
        rerun.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&rerun.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&rerun.stdout),
        full_update_report(&root, "done")
    );
    let current = router_fleet_database(&root);
    assert_eq!(read_update_ledger(&current).await?, completed_ledger(true));
    assert_eq!(
        FleetValues::read_merged(&current).await?,
        fixture_values,
        "every value, including the row that was only in a log, must survive"
    );
    assert_eq!(
        inspect_router_update_requirement(&root).await?,
        RouterUpdateRequirement::NotRequired
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_kill_after_step_0001_is_done_reruns_only_the_later_steps() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;

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
    let report = full_update_report(&root, "done");
    let later_steps = report
        .lines()
        .skip(1)
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    assert_eq!(String::from_utf8_lossy(&rerun.stdout), later_steps);
    assert_eq!(
        read_update_ledger(&router_fleet_database(&root)).await?,
        completed_ledger(true)
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_root_that_never_connected_ends_with_its_fleet_database() -> UpdateResult {
    // Arrange: a root with no Fleet database at all.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    std::fs::create_dir(&root)?;
    let former = router_fleet_cache_database(&root);
    let current = router_fleet_database(&root);

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
            "done 0001: create {former} with the update ledger\n\
             done 0005: no older Fleet store to move\n\
             done 0010: rename {former} to {current}\n",
            former = former.display(),
            current = current.display(),
        )
    );
    assert!(!former.exists());
    assert_eq!(read_update_ledger(&current).await?, completed_ledger(false));
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_fleet_database_no_update_made_stops_the_update_naming_it() -> UpdateResult {
    // Arrange: one root with a foreign fleet.sqlite beside the old cache, one with a
    // fleet.sqlite alone that carries no ledger.
    let (_beside_directory, beside) = populated_root()?;
    let foreign_beside = router_fleet_database(&beside);
    std::fs::write(&foreign_beside, b"not made by agent-router-update")?;
    let alone_directory = tempfile::tempdir()?;
    let alone = alone_directory.path().join("router");
    std::fs::create_dir_all(router_collaboration_directory(&alone))?;
    let foreign_alone = router_fleet_database(&alone);
    sqlx_turso_store_without_ledger(&foreign_alone).await?;
    let before = (snapshot(&beside)?, snapshot(&alone)?);

    // Act
    let both_names = run_update(&root_argument(&beside)).await?;
    let unexplained = run_update(&root_argument(&alone)).await?;

    // Assert
    let both_stderr = String::from_utf8_lossy(&both_names.stderr);
    assert_eq!(both_names.status.code(), Some(2), "{both_stderr}");
    assert!(
        both_stderr.contains(&foreign_beside.display().to_string()),
        "{both_stderr}"
    );
    assert!(
        both_stderr.contains(&router_fleet_cache_database(&beside).display().to_string()),
        "{both_stderr}"
    );
    let alone_stderr = String::from_utf8_lossy(&unexplained.stderr);
    assert_eq!(unexplained.status.code(), Some(2), "{alone_stderr}");
    assert!(alone_stderr.contains("no update made it"), "{alone_stderr}");
    assert_eq!((snapshot(&beside)?, snapshot(&alone)?), before);
    Ok(())
}

async fn sqlx_turso_store_without_ledger(path: &Path) -> UpdateResult {
    use sqlx::{ConnectOptions, Connection, Executor};
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .connect()
        .await?;
    connection
        .execute("CREATE TABLE unrelated(value TEXT)")
        .await?;
    connection.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_host_stopped_before_recording_its_new_store_is_refused_until_the_update_completes()
-> UpdateResult {
    // Arrange: the state a Host leaves when it dies after creating and migrating its new
    // Fleet database but before recording the catalog done: the ledger table, no rows.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    std::fs::create_dir_all(router_collaboration_directory(&root))?;
    let store = router_fleet_database(&root);
    UpdateLedger::create(&store).await?.close().await;

    // Act
    let before = inspect_router_update_requirement(&root).await?;
    let update = run_update(&root_argument(&root)).await?;
    let after = inspect_router_update_requirement(&root).await?;

    // Assert: refused, then the tool records each step as a no-op, then the Host may start.
    assert_eq!(before, unfinished(&store, &[1, 5, 10]));
    assert_eq!(
        update.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&update.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&update.stdout),
        format!(
            "done 0001: add the update ledger to {store}\n\
             done 0005: no older Fleet store to move\n\
             done 0010: {store} already has its current name\n",
            store = store.display(),
        )
    );
    assert_eq!(read_update_ledger(&store).await?, completed_ledger(false));
    assert_eq!(after, RouterUpdateRequirement::NotRequired);
    Ok(())
}
