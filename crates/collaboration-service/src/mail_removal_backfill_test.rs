//! Existing, non-journaled custody receives the real SDK removal task once.
use super::*;
use durable_steps::storage::TransactionScope;
use fleet_sync_delivery::delivery_rollback;
use runtime_primitives::SyncTime;
use std::{
    os::unix::{fs::PermissionsExt, process::ExitStatusExt},
    path::Path,
    process::Stdio,
};
use tokio::io::AsyncReadExt;

async fn seed_legacy_item(runtime: &Runtime) -> fleet_sync_protocol::ItemId {
    let item = runtime.fixture.item(&runtime.fixture.draft());
    let id = item.id();
    let config = runtime.fixture.host.config.local;
    let consumer = runtime.fixture.host.consumer.clone();
    runtime
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                // The public pre-journal API deliberately leaves this row without removal work.
                Inbox::<_, collaboration_contracts::RouterDeliveryTypes>::new(
                    scope,
                    config.owner,
                    config.limits,
                )
                .enqueue_local(
                    item,
                    consumer.as_ref(),
                    SyncTime::from_unix_millis(recorded_at().timestamp_millis()),
                )
                .await
                .map_err(delivery_rollback)?;
                Ok(())
            })
        })
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn startup_backfills_legacy_custody_once_and_retains_it_until_day60() {
    // Arrange legacy custody with no removal operation, then cold-open its actual owner.
    let runtime = Runtime::new().await;
    let item = seed_legacy_item(&runtime).await;
    let runtime = runtime.reopen().await;
    let mail = runtime.local_mail_runtime();
    // Act through the owning runner's real startup, with cancellation already requested.
    let shutdown = tokio_util::sync::CancellationToken::new();
    shutdown.cancel();
    mail.run(shutdown).await.unwrap();
    assert_eq!(
        runtime
            .sql_count("SELECT count(*) AS count FROM mail_removal_backfill WHERE admitted_count=1")
            .await,
        1
    );
    assert_eq!(mail.initialize_record_removals().await.unwrap(), None);
    drop(mail);
    let runtime = runtime.reopen().await;
    let mail = runtime.local_mail_runtime();
    assert_eq!(mail.initialize_record_removals().await.unwrap(), None);
    // Assert using the independently specified UUIDv7 deadline, through the real engine.
    let created = item.as_uuid().as_bytes()[..6]
        .iter()
        .fold(0_i64, |time, byte| (time << 8) | i64::from(*byte));
    for (day, expected) in [(59, 1), (60, 0)] {
        runtime.fixture.host.clock.store(
            created + day * 86_400_000,
            std::sync::atomic::Ordering::SeqCst,
        );
        mail.drive_due_for_test().await.unwrap();
        assert_eq!(
            runtime
                .sql_item_count(
                    "SELECT count(*) AS count FROM _fleet_sync_inbox_items WHERE item_id=?",
                    item
                )
                .await,
            expected
        );
    }
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn completed_backfill_skips_the_sdk_custody_scan() {
    // Arrange a completed pass and a legacy key that would make any subsequent scan fail.
    let runtime = Runtime::new().await;
    let mail = runtime.local_mail_runtime();
    assert_eq!(mail.initialize_record_removals().await.unwrap(), Some(0));
    let item = seed_legacy_item(&runtime).await;
    runtime.fixture.store.transaction(move |scope| Box::pin(async move {
        sqlx::query("UPDATE _fleet_sync_inbox_items SET item_id='invalid-legacy-key' WHERE item_id=?")
            .bind(item.as_uuid().to_string()).execute(scope.executor()).await?;
        Ok(())
    })).await.unwrap();
    // Act and assert: the durable marker must short-circuit before the SDK inspects keys.
    assert_eq!(mail.initialize_record_removals().await.unwrap(), None);
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn corrupt_completion_marker_refuses_startup_instead_of_skipping() {
    // Arrange the real completed marker, then corrupt each independently validated field.
    let runtime = Runtime::new().await;
    let mail = runtime.local_mail_runtime();
    assert_eq!(mail.initialize_record_removals().await.unwrap(), Some(0));
    for corrupt in [
        "UPDATE mail_removal_backfill SET admitted_count=-1",
        "UPDATE mail_removal_backfill SET id='00000000-0000-4000-8000-000000000001'",
        "UPDATE mail_removal_backfill SET created_at=-1",
        "UPDATE mail_removal_backfill SET updated_at=created_at+1",
    ] {
        runtime.fixture.store.transaction(move |scope| Box::pin(async move {
            let valid_id = uuid::Uuid::now_v7().to_string();
            sqlx::query("UPDATE mail_removal_backfill SET id=?,admitted_count=0,created_at=1,updated_at=1")
                .bind(valid_id).execute(scope.executor()).await?;
            sqlx::query(corrupt).execute(scope.executor()).await?;
            Ok(())
        })).await.unwrap();
        // Act and assert: malformed persisted completion is never treated as completed.
        assert!(
            mail.initialize_record_removals().await.is_err(),
            "accepted corrupt marker: {corrupt}"
        );
    }
    drop(mail);
    runtime.close().await;
}

const CHILD_TEST: &str = "mail_push_delivery::tests::removal_backfill::backfill_process_child";
const CHILD_ROOT: &str = "CODEX_ROUTER_TEST_BACKFILL_ROOT";

#[tokio::test]
#[ignore = "Owned precommit crash child; parent supplies its private directory"]
async fn backfill_process_child() {
    let root = std::path::PathBuf::from(std::env::var_os(CHILD_ROOT).unwrap());
    let runtime = Runtime::new().await;
    let item = seed_legacy_item(&runtime).await;
    std::fs::write(
        root.join("database-path"),
        runtime
            .fixture
            .database_path()
            .as_os_str()
            .as_encoded_bytes(),
    )
    .unwrap();
    std::fs::write(
        root.join("peer"),
        runtime
            .fixture
            .host
            .config
            .local
            .owner
            .as_uuid()
            .to_string(),
    )
    .unwrap();
    std::fs::write(root.join("item"), item.as_uuid().to_string()).unwrap();
    let mut mail = runtime.local_mail_runtime();
    mail.backfill_admission_probe = Some(root.join("held.sock"));
    mail.initialize_record_removals().await.unwrap();
    panic!("precommit backfill escaped its causal hold");
}

async fn stored_count(store: &ProjectionStore, statement: &'static str) -> i64 {
    store
        .transaction(move |scope| {
            Box::pin(async move {
                Ok(sqlx::query_scalar::<_, i64>(statement)
                    .fetch_one(scope.executor())
                    .await?)
            })
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn sigkill_before_backfill_commit_retries_both_marker_and_admissions() {
    // Arrange a real child-owned transaction held after both admissions and marker writes.
    let root = tempfile::Builder::new()
        .prefix("c5back-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let child_temp = root.path().join("child");
    std::fs::create_dir(&child_temp).unwrap();
    let listener = tokio::net::UnixListener::bind(root.path().join("held.sock")).unwrap();
    let log = std::fs::File::create(root.path().join("child.log")).unwrap();
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", CHILD_TEST, "--nocapture"])
        .env(CHILD_ROOT, root.path())
        .env("TMPDIR", &child_temp)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let ready = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut marker = [0; 1];
        stream.read_exact(&mut marker).await.unwrap();
        assert_eq!(&marker, b"R");
        stream
    })
    .await;
    // Act: interrupt the actual transaction, with no cancellation stand-in.
    child.kill().await.unwrap();
    let status = child.wait().await.unwrap();
    assert_eq!(status.signal(), Some(9));
    drop(ready.unwrap_or_else(|error| {
        panic!(
            "backfill checkpoint: {error}; {}",
            std::fs::read_to_string(root.path().join("child.log")).unwrap()
        )
    }));
    let database = std::fs::read_to_string(root.path().join("database-path")).unwrap();
    assert!(Path::new(&database).starts_with(&child_temp));
    let peer = fleet_sync_protocol::PeerId::try_from(
        std::fs::read_to_string(root.path().join("peer")).unwrap(),
    )
    .unwrap();
    let item = fleet_sync_protocol::ItemId::try_from(
        std::fs::read_to_string(root.path().join("item")).unwrap(),
    )
    .unwrap();
    let owner = Arc::new(
        interaction_storage::InteractionStore::open(&database)
            .await
            .unwrap(),
    );
    let runtime = Runtime::new().await;
    let store = interaction_storage::InteractionAtomicStore::new(
        owner.clone(),
        runtime.fixture.host.clone(),
    );
    // Assert WAL recovery kept legacy custody, but committed neither half of the upgrade.
    assert_eq!(
        stored_count(&store, "SELECT count(*) FROM mail_removal_backfill").await,
        0
    );
    assert_eq!(stored_count(&store, "SELECT count(*) FROM _fleet_sync_operations WHERE kind='fleet-sync.delivery.remove-record'").await, 0);
    assert_eq!(
        stored_count(&store, "SELECT count(*) FROM _fleet_sync_inbox_items").await,
        1
    );
    let mut props = runtime.local_mail_props();
    props.interactions = owner.clone();
    let mail = crate::MailDeliveryRuntime::new(peer, props, None).unwrap();
    assert_eq!(mail.initialize_record_removals().await.unwrap(), Some(1));
    assert_eq!(mail.initialize_record_removals().await.unwrap(), None);
    assert_eq!(
        stored_count(
            &store,
            "SELECT count(*) FROM mail_removal_backfill WHERE admitted_count=1"
        )
        .await,
        1
    );
    assert_eq!(stored_count(&store, "SELECT count(*) FROM _fleet_sync_operations WHERE kind='fleet-sync.delivery.remove-record'").await, 1);
    // The SDK row still belongs to the exact original item frozen before the crash.
    let retained_id = store
        .transaction(move |scope| {
            Box::pin(async move {
                Ok(
                    sqlx::query_scalar::<_, String>("SELECT item_id FROM _fleet_sync_inbox_items")
                        .fetch_one(scope.executor())
                        .await?,
                )
            })
        })
        .await
        .unwrap();
    assert_eq!(retained_id, item.as_uuid().to_string());
    drop(mail);
    drop(store);
    Arc::try_unwrap(owner).ok().unwrap().close().await.unwrap();
    runtime.close().await;
}
