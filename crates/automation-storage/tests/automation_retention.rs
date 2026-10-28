//! Automation retains its own mailbox deliveries and receipts after the message cutover.

use automation_storage::AutomationStore;
use sqlx::{Connection, sqlite::SqliteConnectOptions};
use std::path::PathBuf;

struct TestDatabase {
    path: PathBuf,
}

impl TestDatabase {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!(
                "automation-push-records-{}.sqlite",
                uuid::Uuid::now_v7()
            )),
        }
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[tokio::test]
async fn migration_removes_legacy_message_and_latest_sender_tables() {
    let database = TestDatabase::new();
    let store = AutomationStore::open(&database.path)
        .await
        .expect("automation store migrates");
    store.close().await.expect("close migrated store");

    let mut observer = sqlx::SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&database.path)
            .foreign_keys(true),
    )
    .await
    .expect("open isolated schema observer");
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('router_pushes', 'latest_agent_senders') ORDER BY name",
    )
    .fetch_all(&mut observer)
    .await
    .expect("read migrated table inventory");

    assert!(tables.is_empty());
    observer.close().await.expect("close schema observer");
}

async fn insert_mailbox_fixture(
    observer: &mut sqlx::SqliteConnection,
    wake_id: &str,
    delivery_id: &str,
    created_at_ms: i64,
    delivery_status: &str,
    pending: bool,
) -> Result<(), sqlx::Error> {
    let change_id = uuid::Uuid::now_v7().to_string();
    let occurrence_id = uuid::Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO wakeup_definitions (wakeup_id,change_id,wakeup_status,definition_json,anchor_at_ms,evaluated_through_ms,next_due_at_ms,expires_at_ms,first_fire_json,pending_delivery_id,created_at_ms,updated_at_ms) VALUES (?,?,'active','{}',?,?,NULL,NULL,NULL,NULL,?,?)",
    )
    .bind(wake_id)
    .bind(change_id)
    .bind(created_at_ms)
    .bind(created_at_ms)
    .bind(created_at_ms)
    .bind(created_at_ms)
    .execute(&mut *observer)
    .await?;
    sqlx::query(
        "INSERT INTO mailbox_deliveries (delivery_id,wakeup_id,occurrence_id,due_at_ms,fired_at_ms,target_json,message_json,delivery_mode,generation_guard_json,delivery_status,eligible_at_ms,expires_at_ms,latest_attempt_json,outcome_receipt_json,created_at_ms) VALUES (?,?,?, ?, ?, '{}','{}','queue',NULL,?,?,NULL,NULL,NULL,?)",
    )
    .bind(delivery_id)
    .bind(wake_id)
    .bind(occurrence_id)
    .bind(created_at_ms)
    .bind(created_at_ms)
    .bind(delivery_status)
    .bind(created_at_ms)
    .bind(created_at_ms)
    .execute(&mut *observer)
    .await?;
    if pending {
        sqlx::query("UPDATE wakeup_definitions SET pending_delivery_id=? WHERE wakeup_id=?")
            .bind(delivery_id)
            .bind(wake_id)
            .execute(&mut *observer)
            .await?;
    }
    Ok(())
}

#[tokio::test]
async fn thirty_day_cleanup_deletes_whole_settled_mailbox_and_operation_receipts() {
    let database = TestDatabase::new();
    let mut store = AutomationStore::open(&database.path)
        .await
        .expect("automation store migrates");
    let mut observer = sqlx::SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&database.path)
            .foreign_keys(true),
    )
    .await
    .expect("open isolated retention observer");
    let now_ms = 1_700_000_000_000_i64;
    let cutoff_ms = now_ms - 30 * 24 * 60 * 60 * 1000;
    let expired_ms = cutoff_ms - 1;
    let expired_delivery_id = uuid::Uuid::now_v7().to_string();
    let boundary_delivery_id = uuid::Uuid::now_v7().to_string();
    let recent_delivery_id = uuid::Uuid::now_v7().to_string();
    let pending_delivery_id = uuid::Uuid::now_v7().to_string();

    for (created_at_ms, status, pending, delivery_id) in [
        (expired_ms, "accepted", false, expired_delivery_id.as_str()),
        (cutoff_ms, "accepted", false, boundary_delivery_id.as_str()),
        (now_ms - 1, "accepted", false, recent_delivery_id.as_str()),
        (expired_ms, "pending", true, pending_delivery_id.as_str()),
    ] {
        let wake_id = uuid::Uuid::now_v7().to_string();
        insert_mailbox_fixture(
            &mut observer,
            &wake_id,
            delivery_id,
            created_at_ms,
            status,
            pending,
        )
        .await
        .expect("insert mailbox fixture");
    }

    let expired_receipt_id = uuid::Uuid::now_v7().to_string();
    let boundary_receipt_id = uuid::Uuid::now_v7().to_string();
    let recent_receipt_id = uuid::Uuid::now_v7().to_string();
    for (operation_id, committed_at_ms) in [
        (expired_receipt_id.as_str(), expired_ms),
        (boundary_receipt_id.as_str(), cutoff_ms),
        (recent_receipt_id.as_str(), now_ms - 1),
    ] {
        sqlx::query(
            "INSERT INTO operation_receipts (operation_id,method_name,canonical_request,resource_id,operation_status,effect_evidence_json,final_result_json,final_error_json,committed_at_ms) VALUES (?,'message/send',x'01','resource','succeeded','{}',NULL,NULL,?)",
        )
        .bind(operation_id)
        .bind(committed_at_ms)
        .execute(&mut observer)
        .await
        .expect("insert whole receipt fixture");
    }
    observer.close().await.expect("close retention observer");

    assert_eq!(
        store
            .prune_settled_mailbox_deliveries(now_ms, 500)
            .await
            .expect("prune settled mailbox rows"),
        1
    );
    assert_eq!(
        store
            .prune_operation_receipts(now_ms, 500)
            .await
            .expect("prune whole operation receipts"),
        1
    );
    let mut observer = sqlx::SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&database.path)
            .foreign_keys(true),
    )
    .await
    .expect("reopen retention observer");
    for (delivery_id, expected_count) in [
        (expired_delivery_id.as_str(), 0_i64),
        (boundary_delivery_id.as_str(), 1),
        (recent_delivery_id.as_str(), 1),
        (pending_delivery_id.as_str(), 1),
    ] {
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM mailbox_deliveries WHERE delivery_id=?")
                .bind(delivery_id)
                .fetch_one(&mut observer)
                .await
                .expect("read mailbox retention result");
        assert_eq!(count, expected_count);
    }
    for (operation_id, expected_count) in [
        (expired_receipt_id.as_str(), 0_i64),
        (boundary_receipt_id.as_str(), 1),
        (recent_receipt_id.as_str(), 1),
    ] {
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM operation_receipts WHERE operation_id=?")
                .bind(operation_id)
                .fetch_one(&mut observer)
                .await
                .expect("read operation retention result");
        assert_eq!(count, expected_count);
    }
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut observer)
            .await
            .expect("check retained foreign keys")
            .is_empty()
    );
    observer.close().await.expect("close retention observer");
    store.close().await.expect("close automation store");
}
