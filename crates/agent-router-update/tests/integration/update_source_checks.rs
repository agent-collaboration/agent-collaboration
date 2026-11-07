//! J4 (source checks): step 0005 reads an older store only through the decoders that own
//! its values, and never reads a store again once its rows have moved.
//!
//! A well-formed store holding one value its owner refuses stops the update before any
//! store is carried or removed, naming the store, table and row. So does a cursor key
//! other than the one the Fleet database already holds, naming both stores. A store that
//! reappears after its rows moved is removed without being opened, and the Fleet database
//! is left byte for byte as it was.
use crate::update_ledger_journey::root_argument;
use crate::update_moves_journey::{assert_update_finished, fixture_values};
use crate::update_support::{
    CREATION_REQUEST, FleetValues, PENDING_REQUEST, UpdateResult, populated_root, run_update,
    run_update_killed_at, snapshot, write_store_value,
};
use codex_router_host::{
    router_collaboration_directory, router_fleet_cache_database, router_fleet_database,
};
use message_board_storage::{
    StoredUpdateLedger, UpdateLedgerEntry, UpdateStepNumber, UpdateStepProgress, read_update_ledger,
};
use std::path::Path;

const LEGACY_STORE_FILES: [&str; 4] = [
    "fleet-outbox.sqlite",
    "fleet-outbox.sqlite-wal",
    "board-cursors.sqlite",
    "board-cursors.sqlite-wal",
];

/// Gives one older store an invalid value, runs the update, and checks it refused naming
/// `store`, `table` and `key` while leaving every older store and the merge untouched.
async fn assert_invalid_value_refused(
    store: &str,
    table: &str,
    key: impl Fn(&FleetValues) -> String,
    statement: impl Fn(&str) -> String,
) -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let directory = router_collaboration_directory(&root);
    let key = key(&fixture_values(&root).await?);
    write_store_value(&directory.join(store), &statement(&key)).await?;
    let stores_before = snapshot(&directory)?;

    // Act
    let refused = run_update(&root_argument(&root)).await?;

    // Assert: the refusal names the store and the row; 0001 stays done and nothing of
    // 0005 happened, so no store was carried, removed or renamed.
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains(&directory.join(store).display().to_string())
            && stderr.contains(&format!(
                "{store} has an invalid row in {table} under key {key}"
            )),
        "the refusal must name {store}, {table} and {key}: {stderr}"
    );
    let former = router_fleet_cache_database(&root);
    assert_eq!(
        read_update_ledger(&former).await?,
        StoredUpdateLedger::Present(vec![UpdateLedgerEntry::Step {
            step: UpdateStepNumber::new(1),
            progress: UpdateStepProgress::Done,
        }])
    );
    let merged = FleetValues::read_merged(&former).await?;
    assert!(
        merged.requests.is_empty()
            && merged.creation_inputs.is_empty()
            && merged.cursor_keys.is_empty(),
        "nothing may be carried: {merged:?}"
    );
    let stores_after = snapshot(&directory)?;
    for file in LEGACY_STORE_FILES {
        assert_eq!(
            stores_after.get(Path::new(file)),
            stores_before.get(Path::new(file)),
            "{file} must be untouched"
        );
    }
    assert!(
        !router_fleet_database(&root).exists(),
        "nothing was renamed"
    );
    Ok(())
}

#[tokio::test]
async fn a_cursor_key_of_the_wrong_length_stops_the_update_before_any_store_moves() -> UpdateResult
{
    // The cursor store is checked after the outbox in the loop, so this also shows the
    // outbox is not carried before every store has been checked.
    assert_invalid_value_refused(
        "board-cursors.sqlite",
        "cursor_signing_key",
        |values| {
            values
                .cursor_keys
                .first()
                .map(|row| row.0.clone())
                .unwrap_or_default()
        },
        |key| format!("UPDATE cursor_signing_key SET key_bytes = x'00' WHERE key_id = '{key}'"),
    )
    .await
}

#[tokio::test]
async fn an_outbox_request_with_an_unknown_state_stops_the_update_before_any_store_moves()
-> UpdateResult {
    assert_invalid_value_refused(
        "fleet-outbox.sqlite",
        "fleet_outbox_requests",
        |_| PENDING_REQUEST.to_owned(),
        |key| {
            format!(
                "UPDATE fleet_outbox_requests SET state = '{{\"kind\":\"unknown\"}}' WHERE request_id = '{key}'"
            )
        },
    )
    .await
}

#[tokio::test]
async fn a_creation_input_that_no_longer_matches_its_hash_stops_the_update_before_any_store_moves()
-> UpdateResult {
    assert_invalid_value_refused(
        "fleet-outbox.sqlite",
        "project_creation_inputs",
        |_| CREATION_REQUEST.to_owned(),
        |key| {
            format!(
                "UPDATE project_creation_inputs SET input = json_set(input, '$.title', 'a different title') WHERE request_id = '{key}'"
            )
        },
    )
    .await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_store_that_reappears_after_the_update_is_removed_unread() -> UpdateResult {
    // Arrange: a finished update, then a malformed cursor store and a stray outbox log
    // back in the directory. Opening either as a database would fail.
    let (_directory, root) = populated_root()?;
    let expected = fixture_values(&root).await?;
    let finished = run_update(&root_argument(&root)).await?;
    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
    let directory = router_collaboration_directory(&root);
    let cursors = directory.join("board-cursors.sqlite");
    let outbox = directory.join("fleet-outbox.sqlite");
    std::fs::write(&cursors, b"this is not a database")?;
    std::fs::write(
        directory.join("fleet-outbox.sqlite-wal"),
        b"nor is this a log",
    )?;
    let before = snapshot(&directory)?;
    let stale_report = |verb: &str| {
        format!(
            "{verb} stale store: remove {} unread; its rows were already moved\n\
             {verb} stale store: remove {} unread; its rows were already moved\n",
            outbox.display(),
            cursors.display()
        )
    };

    // Act and assert: a dry run lists the cleanup and changes nothing.
    let dry_run = run_update(&[
        root_argument(&root)[0],
        root_argument(&root)[1],
        "--dry-run".as_ref(),
    ])
    .await?;
    assert_eq!(dry_run.status.code(), Some(0), "{dry_run:?}");
    assert_eq!(
        String::from_utf8_lossy(&dry_run.stdout),
        format!("{}dry run: nothing was changed\n", stale_report("pending"))
    );
    assert_eq!(snapshot(&directory)?, before, "a dry run changes nothing");

    // Act
    let cleanup = run_update(&root_argument(&root)).await?;

    // Assert: the stale files are gone; every other file, the Fleet database among
    // them, is byte for byte what it was.
    assert_eq!(cleanup.status.code(), Some(0), "{cleanup:?}");
    assert_eq!(
        String::from_utf8_lossy(&cleanup.stdout),
        stale_report("done")
    );
    let mut expected_files = before;
    expected_files.remove(Path::new("board-cursors.sqlite"));
    expected_files.remove(Path::new("fleet-outbox.sqlite-wal"));
    assert_eq!(snapshot(&directory)?, expected_files);
    assert_update_finished(&root, &expected, "after stale cleanup").await?;
    let again = run_update(&root_argument(&root)).await?;
    assert_eq!(
        String::from_utf8_lossy(&again.stdout),
        format!("{} needs no update\n", root.display())
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_store_that_reappears_after_its_own_removal_is_removed_unread_mid_merge() -> UpdateResult
{
    // Arrange: killed once the outbox is recorded removed but before step 0005 is done;
    // then a malformed outbox store appears under the removed name.
    let (_directory, root) = populated_root()?;
    let expected = fixture_values(&root).await?;
    run_update_killed_at(&root, "step-0005:fleet-outbox.sqlite:removed").await?;
    let outbox = router_collaboration_directory(&root).join("fleet-outbox.sqlite");
    std::fs::write(&outbox, b"this is not a database")?;

    // Act
    let rerun = run_update(&root_argument(&root)).await?;

    // Assert
    assert_eq!(
        rerun.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&rerun.stderr)
    );
    assert!(!outbox.exists(), "the stale outbox store must be removed");
    assert_update_finished(&root, &expected, "stale store mid-merge").await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_fleet_database_holding_another_cursor_key_stops_the_update_naming_both_stores()
-> UpdateResult {
    // Arrange: step 0001 is done, then the Fleet database gains a cursor key of its own,
    // different from the one board-cursors.sqlite holds.
    let (_directory, root) = populated_root()?;
    run_update_killed_at(&root, "step-0001:done").await?;
    let former = router_fleet_cache_database(&root);
    let held_key = (
        "019f0000-0000-7000-8000-00000000d399".to_owned(),
        vec![0xe7_u8; 32],
    );
    write_store_value(
        &former,
        &format!(
            "INSERT INTO fleet_cursor_signing_key(key_id, key_bytes) VALUES ('{}', x'{}')",
            held_key.0,
            "e7".repeat(32)
        ),
    )
    .await?;
    let directory = router_collaboration_directory(&root);
    let cursors = directory.join("board-cursors.sqlite");
    let stores_before = snapshot(&directory)?;

    // Act
    let refused = run_update(&root_argument(&root)).await?;

    // Assert: both stores are named, and neither changed: the Fleet database still holds
    // only its own key and nothing was carried, and every older store is untouched.
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "{} and {} hold different board cursor signing keys",
            cursors.display(),
            former.display()
        )),
        "the refusal must name both stores: {stderr}"
    );
    let merged = FleetValues::read_merged(&former).await?;
    assert_eq!(merged.cursor_keys, vec![held_key]);
    assert!(
        merged.requests.is_empty() && merged.creation_inputs.is_empty(),
        "nothing may be carried: {merged:?}"
    );
    let stores_after = snapshot(&directory)?;
    for file in LEGACY_STORE_FILES {
        assert_eq!(
            stores_after.get(Path::new(file)),
            stores_before.get(Path::new(file)),
            "{file} must be untouched"
        );
    }
    assert!(
        !router_fleet_database(&root).exists(),
        "nothing was renamed"
    );
    Ok(())
}
