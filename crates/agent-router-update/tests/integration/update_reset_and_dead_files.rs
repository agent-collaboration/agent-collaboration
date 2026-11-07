//! J4 (reset and dead files): step 0002 finishes an older Router's enrollment reset
//! against the Fleet database under its former name, before any store moves; step 0009
//! removes the files nothing reads, importing approval routes first; the Host refuses to
//! start while a dead file remains, and a dead file found after step 0009 is removed
//! unread.
use crate::update_ledger_journey::root_argument;
use crate::update_moves_journey::{assert_update_finished, fixture_values};
use crate::update_support::{
    FleetValues, UpdateResult, populated_root, run_update, run_update_inspecting_at, snapshot,
};
use codex_router_host::{
    RequiredRouterUpdate, RouterUpdateRequirement, inspect_router_update_requirement,
    router_collaboration_directory, router_fleet_cache_database, router_fleet_database,
};
use std::path::{Path, PathBuf};

const RESET_GENERATION: &str = "019f0000-0000-7000-8000-00000000d3a1";
const ROUTE_THREAD: &str = "019f0000-0000-7000-8000-00000000d3e1";

/// Writes the journal an older Router keeps its enrollment reset in, in `phase`.
fn write_reset_journal(root: &Path, phase: &str) -> UpdateResult<PathBuf> {
    let journal = router_collaboration_directory(root).join("enrollment-reset.json");
    std::fs::write(
        &journal,
        format!(r#"{{"generation":"{RESET_GENERATION}","phase":{{"kind":"{phase}"}}}}"#),
    )?;
    Ok(journal)
}

/// One approval route in the JSON shape an older Router wrote.
fn legacy_approval_routes() -> String {
    let session = |id: &str| {
        format!(
            r#"{{"endpoint":{{"serviceId":"019f0000-0000-7000-8000-00000000d3e0","endpointId":"codex-local"}},"sessionId":"{id}"}}"#
        )
    };
    format!(
        r#"[{{"threadId":"{ROUTE_THREAD}","createdBy":{},"approver":{},"access":"workspace-write","scratchPath":"/tmp/d3-route-scratch","rootMessageId":null}}]"#,
        session("creator"),
        session("approver")
    )
}

/// The thread ids `interaction.sqlite` holds approval routes for, read only.
async fn imported_route_threads(root: &Path) -> UpdateResult<Vec<String>> {
    use sqlx::ConnectOptions;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(router_collaboration_directory(root).join("interaction.sqlite"))
        .read_only(true)
        .connect()
        .await?;
    let threads = sqlx::query_scalar::<sqlx_turso::Turso, String>(
        "SELECT thread_id FROM approval_routes ORDER BY thread_id",
    )
    .fetch_all(&mut connection)
    .await?;
    sqlx::Connection::close(connection).await?;
    Ok(threads)
}

/// Every dead file an older Router may leave, an export folder, and a valid routes file.
fn write_dead_files(root: &Path) -> UpdateResult<Vec<PathBuf>> {
    let directory = router_collaboration_directory(root);
    let export = directory.join("native-schema-export-0001");
    std::fs::create_dir_all(&export)?;
    std::fs::write(export.join("schema.json"), b"{}")?;
    let mut paths = vec![export];
    for name in [
        "approval-history.json",
        "control-schema-ab12.json",
        "interaction-history.json",
        "project-board.sqlite",
        "project-board.sqlite-wal",
    ] {
        std::fs::write(directory.join(name), name)?;
        paths.push(directory.join(name));
    }
    let routes = directory.join("approval-routes.json");
    std::fs::write(&routes, legacy_approval_routes())?;
    paths.push(routes);
    paths.sort();
    Ok(paths)
}

fn names_in(directory: &Path) -> UpdateResult<Vec<String>> {
    let mut names = std::fs::read_dir(directory)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect::<UpdateResult<Vec<_>>>()?;
    names.sort();
    Ok(names)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_pending_reset_is_finished_against_the_former_store_before_any_store_moves()
-> UpdateResult {
    // Arrange: a pending reset, a learned session cursor, and the fixture's cache rows.
    let (_directory, root) = populated_root()?;
    let mut expected = fixture_values(&root).await?;
    let directory = router_collaboration_directory(&root);
    let journal = write_reset_journal(&root, "pending")?;
    std::fs::create_dir_all(directory.join("machine-session"))?;
    std::fs::write(directory.join("machine-session/state.json"), b"old cursor")?;
    let former = router_fleet_cache_database(&root);

    // Act: hold the update right after step 0002 and look at the root.
    let status = run_update_inspecting_at(&root, "step-0002:done", || async {
        let cleared = FleetValues::read_merged(&former).await?;
        assert!(
            cleared.cached_records.is_empty() && cleared.checkpoint == 0,
            "the reset clears the store under its former name: {cleared:?}"
        );
        for store in ["fleet-outbox.sqlite", "board-cursors.sqlite"] {
            assert!(directory.join(store).exists(), "{store} has not moved yet");
        }
        assert!(
            !router_fleet_database(&root).exists(),
            "nothing renamed yet"
        );
        assert!(!directory.join("machine-session/state.json").exists());
        Ok(())
    })
    .await?;

    // Assert: the update finished with the cache cleared and every carried value kept.
    assert!(
        status.success(),
        "the held update must finish once released"
    );
    expected.cached_records.clear();
    expected.checkpoint = 0;
    assert_update_finished(&root, &expected, "pending reset").await?;
    assert!(!journal.exists(), "step 0009 removes the finished journal");
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_reset_past_its_clearing_and_a_completed_one_keep_the_cache_and_lose_the_journal()
-> UpdateResult {
    for phase in ["routerStateCleared", "completed"] {
        // Arrange: the clearing already happened, so the cache now holds new rows.
        let (_directory, root) = populated_root()?;
        let expected = fixture_values(&root).await?;
        let journal = write_reset_journal(&root, phase)?;

        // Act
        let update = run_update(&root_argument(&root)).await?;

        // Assert
        assert_eq!(
            update.status.code(),
            Some(0),
            "{phase}: {}",
            String::from_utf8_lossy(&update.stderr)
        );
        assert_update_finished(&root, &expected, phase).await?;
        assert!(
            !journal.exists(),
            "{phase}: the finished journal is removed"
        );
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn an_unreadable_reset_journal_stops_the_update_before_it_changes_anything() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let journal = router_collaboration_directory(&root).join("enrollment-reset.json");
    std::fs::write(&journal, b"{not a journal")?;
    let before = snapshot(&root)?;

    // Act
    let refused = run_update(&root_argument(&root)).await?;

    // Assert
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains(&journal.display().to_string()), "{stderr}");
    assert_eq!(
        snapshot(&root)?,
        before,
        "the root is left exactly as found"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_full_update_imports_the_routes_and_leaves_only_the_stores_the_router_reads()
-> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let expected = fixture_values(&root).await?;
    let dead = write_dead_files(&root)?;
    let directory = router_collaboration_directory(&root);
    let routes = directory.join("approval-routes.json");

    // Act: a dry run first, then the update.
    let dry_run = run_update(&[
        root_argument(&root)[0],
        root_argument(&root)[1],
        "--dry-run".as_ref(),
    ])
    .await?;
    let update = run_update(&root_argument(&root)).await?;

    // Assert: the dry run names the import and every removal; the update leaves only
    // the Fleet database and the store the routes moved into.
    let removals = dead
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let dry_run_stdout = String::from_utf8_lossy(&dry_run.stdout);
    assert!(
        dry_run_stdout.contains(&format!(
            "pending 0009: import the approval routes in {} into interaction.sqlite, then remove {removals}\n",
            routes.display()
        )),
        "{dry_run_stdout}"
    );
    assert_eq!(
        update.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&update.stderr)
    );
    assert_update_finished(&root, &expected, "dead files").await?;
    assert_eq!(imported_route_threads(&root).await?, vec![ROUTE_THREAD]);
    // The write-ahead logs are the stores' own; no JSON file and no older store is left.
    assert_eq!(
        names_in(&directory)?,
        [
            "fleet.sqlite",
            "fleet.sqlite-wal",
            "interaction.sqlite",
            "interaction.sqlite-wal"
        ]
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn malformed_approval_routes_stop_the_update_and_keep_every_dead_file() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let dead = write_dead_files(&root)?;
    let routes = router_collaboration_directory(&root).join("approval-routes.json");
    std::fs::write(&routes, br#"[{"threadId":"thread","access":"invalid"}]"#)?;

    // Act
    let refused = run_update(&root_argument(&root)).await?;

    // Assert: the routes are refused by name; nothing of step 0009 happened.
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains(&routes.display().to_string()), "{stderr}");
    for path in &dead {
        assert!(path.exists(), "{} must be kept", path.display());
    }
    assert!(
        !router_fleet_database(&root).exists(),
        "nothing was renamed"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_dead_file_back_after_the_update_stops_the_host_and_is_removed_unread() -> UpdateResult {
    // Arrange: a finished update, then a malformed approval history file comes back.
    let (_directory, root) = populated_root()?;
    let finished = run_update(&root_argument(&root)).await?;
    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
    let history = router_collaboration_directory(&root).join("approval-history.json");
    std::fs::write(&history, b"{malformed approval history")?;
    let fleet_before = std::fs::read(router_fleet_database(&root))?;

    // Act
    let refused = inspect_router_update_requirement(&root).await?;
    let cleanup = run_update(&root_argument(&root)).await?;

    // Assert
    assert_eq!(
        refused,
        RouterUpdateRequirement::Required(RequiredRouterUpdate::DeadFileRemains {
            path: history.clone()
        })
    );
    assert_eq!(cleanup.status.code(), Some(0), "{cleanup:?}");
    assert_eq!(
        String::from_utf8_lossy(&cleanup.stdout),
        format!(
            "done stale file: remove {} unread; nothing reads it\n",
            history.display()
        )
    );
    assert!(!history.exists());
    assert_eq!(std::fs::read(router_fleet_database(&root))?, fleet_before);
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
async fn a_never_connected_root_with_approval_routes_needs_the_update_and_keeps_its_routes()
-> UpdateResult {
    // Arrange: no Fleet database at all, only the routes an older Router kept in JSON.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let collaboration = router_collaboration_directory(&root);
    std::fs::create_dir_all(&collaboration)?;
    let routes = collaboration.join("approval-routes.json");
    std::fs::write(&routes, legacy_approval_routes())?;

    // Act
    let before = inspect_router_update_requirement(&root).await?;
    let update = run_update(&root_argument(&root)).await?;

    // Assert
    assert_eq!(
        before,
        RouterUpdateRequirement::Required(RequiredRouterUpdate::DeadFileRemains {
            path: routes.clone()
        })
    );
    assert_eq!(
        update.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&update.stderr)
    );
    assert_eq!(imported_route_threads(&root).await?, vec![ROUTE_THREAD]);
    assert!(!routes.exists());
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
async fn a_reset_journal_the_running_host_writes_does_not_stop_it() -> UpdateResult {
    // Arrange: after the update, a later re-enrollment writes the journal again.
    let (_directory, root) = populated_root()?;
    let finished = run_update(&root_argument(&root)).await?;
    assert_eq!(finished.status.code(), Some(0), "{finished:?}");

    for phase in ["pending", "completed"] {
        write_reset_journal(&root, phase)?;

        // Act
        let requirement = inspect_router_update_requirement(&root).await?;

        // Assert: the journal belongs to the running Host, which finishes it itself.
        assert_eq!(requirement, RouterUpdateRequirement::NotRequired, "{phase}");
    }
    Ok(())
}
