//! J4 (proxy part): step 0008 renames the provider proxy store from `state.sqlite` to
//! `provider-proxy.sqlite`, with its account lock folder. An account, a quota snapshot
//! and a session's routing pin committed only in the store's write-ahead log read back
//! unchanged through the store's own reader; a kill at every step barrier reruns to the
//! same end; a log file still open by a reader stops the rename; both names stop the
//! update before it changes anything; and the Host refuses the former name.
use crate::update_ledger_journey::root_argument;
use crate::update_support::{
    UpdateResult, populated_root, run_update, run_update_killed_at, snapshot,
};
use codex_router_core::{ids::AccountId, provider::Provider};
use codex_router_host::{RouterUpdateRequirement, inspect_router_update_requirement};
use codex_router_state::{
    account::{AccountRecord, AccountStatus},
    provider_proxy_database_path,
    quota_snapshot::{PersistedQuotaSnapshot, QuotaSnapshotSource},
    session_account_affinity::SessionAccountAffinity,
    sqlite::AsyncSqliteStateStore,
};
use message_board_storage::{RouterUpdateStep, read_update_ledger};
use sqlx::Connection as _;
use std::path::{Path, PathBuf};

const ACCOUNT: &str = "acct-update-proof";
const SESSION: &str = "session-update-proof";
const LOCK_FILE: &str = "update-proof.lock";
const LOCK_CONTENT: &[u8] = b"held by an account write before the update";

/// The values seeded into the old store, which must read back unchanged.
pub struct ProxyValues {
    account: AccountRecord,
    snapshot: PersistedQuotaSnapshot,
    affinity: SessionAccountAffinity,
}

fn account_id() -> UpdateResult<AccountId> {
    Ok(AccountId::new(ACCOUNT)?)
}

/// Writes the values with the store's own writer and copies the store into `root` as
/// `state.sqlite` while the writer is still open, so they are only in its log there.
pub async fn seed_old_proxy_store(root: &Path) -> UpdateResult<ProxyValues> {
    let writer_directory = tempfile::tempdir()?;
    let writer_path = writer_directory.path().join("state.sqlite");
    let writer = AsyncSqliteStateStore::open(&writer_path).await?;
    let account = AccountRecord::new(
        Provider::Openai,
        account_id()?,
        "Update proof",
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(3);
    writer.upsert_account(&account).await?;
    let snapshot = PersistedQuotaSnapshot::new(account_id()?, QuotaSnapshotSource::OpenAiEndpoint)
        .with_observed_unix_seconds(1_794_000_000)
        .with_route_band("responses", 4_200)
        .with_reset_unix_seconds(1_794_604_800)
        .with_stale_penalty(false);
    writer.upsert_quota_snapshot(&snapshot).await?;
    let affinity =
        SessionAccountAffinity::new(Provider::Openai, SESSION, account_id()?, 1_794_000_100);
    writer.upsert_session_account_affinity(&affinity).await?;
    let old = root.join("state.sqlite");
    std::fs::copy(&writer_path, &old)?;
    std::fs::copy(
        writer_directory.path().join("state.sqlite-wal"),
        root.join("state.sqlite-wal"),
    )?;
    writer.close().await?;
    let locks = root.join("state.sqlite.credential-locks");
    std::fs::create_dir(&locks)?;
    std::fs::write(locks.join(LOCK_FILE), LOCK_CONTENT)?;
    Ok(ProxyValues {
        account,
        snapshot,
        affinity,
    })
}

/// The seeded rows are in the old store's log, not yet in its database file.
async fn assert_rows_only_in_log(root: &Path) -> UpdateResult {
    let copy = tempfile::tempdir()?;
    let database_only = copy.path().join("database-only.sqlite");
    std::fs::copy(root.join("state.sqlite"), &database_only)?;
    let accounts = match AsyncSqliteStateStore::open_read_only(&database_only).await {
        Ok(store) => {
            let accounts = store.list_accounts().await.unwrap_or_default();
            store.close().await?;
            accounts
        }
        Err(_) => Vec::new(),
    };
    assert!(
        accounts.is_empty(),
        "the account must be only in the log: {accounts:?}"
    );
    Ok(())
}

pub async fn assert_proxy_store_renamed(
    root: &Path,
    values: &ProxyValues,
    context: &str,
) -> UpdateResult {
    let current = provider_proxy_database_path(root);
    for gone in [
        "state.sqlite",
        "state.sqlite-wal",
        "state.sqlite-shm",
        "state.sqlite.credential-locks",
    ] {
        assert!(!root.join(gone).exists(), "{context}: {gone} must be gone");
    }
    for log in ["provider-proxy.sqlite-wal", "provider-proxy.sqlite-shm"] {
        assert!(
            !root.join(log).exists(),
            "{context}: {log} must not exist before any open"
        );
    }
    assert_eq!(
        std::fs::read(
            root.join("provider-proxy.sqlite.credential-locks")
                .join(LOCK_FILE)
        )?,
        LOCK_CONTENT,
        "{context}: the lock folder keeps its files under the new name"
    );
    let store = AsyncSqliteStateStore::open_read_only(&current).await?;
    let accounts = store.list_accounts().await?;
    let snapshot = store
        .load_quota_snapshot_for_route_band(&account_id()?, "responses")
        .await?;
    let affinity = store
        .load_session_account_affinity(Provider::Openai, SESSION)
        .await?;
    store.close().await?;
    assert_eq!(accounts, vec![values.account.clone()], "{context}: account");
    assert_eq!(
        snapshot.as_ref(),
        Some(&values.snapshot),
        "{context}: quota snapshot"
    );
    assert_eq!(
        affinity.as_ref(),
        Some(&values.affinity),
        "{context}: routing pin"
    );
    let ledger = read_update_ledger(&codex_router_host::router_fleet_database(root)).await?;
    assert!(
        ledger.step_done(RouterUpdateStep::RenameProviderProxyStore.number()),
        "{context}: {ledger:?}"
    );
    assert_eq!(
        inspect_router_update_requirement(root).await?,
        RouterUpdateRequirement::NotRequired,
        "{context}"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn the_proxy_store_and_its_lock_folder_are_renamed_with_every_logged_row() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let values = seed_old_proxy_store(&root).await?;
    assert_rows_only_in_log(&root).await?;
    let refused = inspect_router_update_requirement(&root).await?;

    // Act
    let output = run_update(&root_argument(&root)).await?;

    // Assert
    assert!(
        matches!(refused, RouterUpdateRequirement::Required(_)),
        "{refused:?}"
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let current = provider_proxy_database_path(&root);
    let expected_line = format!(
        "done 0008: checkpoint and rename {} to {}, then rename {} to {}\n",
        root.join("state.sqlite").display(),
        current.display(),
        root.join("state.sqlite.credential-locks").display(),
        root.join("provider-proxy.sqlite.credential-locks")
            .display()
    );
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains(&expected_line), "{report}");
    assert_proxy_store_renamed(&root, &values, "full update").await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_kill_at_every_step_0008_barrier_reruns_to_the_same_renamed_store() -> UpdateResult {
    for marker in [
        "step-0008:checkpointed",
        "step-0008:store-renamed",
        "step-0008:lock-folder-renamed",
        "step-0008:done",
    ] {
        // Arrange
        let (_directory, root) = populated_root()?;
        let values = seed_old_proxy_store(&root).await?;

        // Act
        run_update_killed_at(&root, marker).await?;
        let refused = inspect_router_update_requirement(&root).await?;
        let rerun = run_update(&root_argument(&root)).await?;

        // Assert
        assert!(
            matches!(refused, RouterUpdateRequirement::Required(_)),
            "killed at {marker}: guard said {refused:?}"
        );
        assert_eq!(
            rerun.status.code(),
            Some(0),
            "killed at {marker}: {}",
            String::from_utf8_lossy(&rerun.stderr)
        );
        assert_proxy_store_renamed(&root, &values, &format!("killed at {marker}")).await?;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_log_kept_open_by_a_reader_stops_the_rename_and_keeps_the_store_under_its_old_name()
-> UpdateResult {
    // Arrange: a reader holds the old store open in a read transaction, so the
    // checkpoint can't empty the log and the shared-memory file stays.
    let (_directory, root) = populated_root()?;
    let _values = seed_old_proxy_store(&root).await?;
    let old = root.join("state.sqlite");
    let mut reader = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&old)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await?;
    sqlx::raw_sql("BEGIN; SELECT count(*) FROM sqlite_master;")
        .execute(&mut reader)
        .await?;

    // Act
    let output = run_update(&root_argument(&root)).await?;

    // Assert
    sqlx::raw_sql("COMMIT;").execute(&mut reader).await?;
    reader.close().await?;
    assert_ne!(output.status.code(), Some(0));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains(&old.display().to_string()), "{error}");
    assert!(old.is_file(), "the store keeps its old name");
    assert!(!provider_proxy_database_path(&root).exists());
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn both_proxy_store_names_stop_the_update_naming_both() -> UpdateResult {
    // Arrange
    let (_directory, root) = populated_root()?;
    let _values = seed_old_proxy_store(&root).await?;
    let current: PathBuf = provider_proxy_database_path(&root);
    std::fs::write(&current, b"")?;
    let before = snapshot(&root)?;

    // Act
    let output = run_update(&root_argument(&root)).await?;

    // Assert
    assert_ne!(output.status.code(), Some(0));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains(&current.display().to_string()), "{error}");
    assert!(
        error.contains(&root.join("state.sqlite").display().to_string()),
        "{error}"
    );
    assert_eq!(snapshot(&root)?, before, "a refused update changes nothing");
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn the_host_refuses_a_proxy_store_left_under_its_former_name_even_after_every_step()
-> UpdateResult {
    // Arrange: an updated root whose every step is done, then the former name reappears.
    let (_directory, root) = populated_root()?;
    let updated = run_update(&root_argument(&root)).await?;
    assert_eq!(
        updated.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&updated.stderr)
    );
    assert_eq!(
        inspect_router_update_requirement(&root).await?,
        RouterUpdateRequirement::NotRequired
    );
    let former = root.join("state.sqlite");
    std::fs::write(&former, b"")?;

    // Act
    let requirement = inspect_router_update_requirement(&root).await?;

    // Assert
    assert_eq!(
        requirement,
        RouterUpdateRequirement::Required(
            codex_router_host::RequiredRouterUpdate::LegacyProviderProxyStore { path: former }
        )
    );
    Ok(())
}
