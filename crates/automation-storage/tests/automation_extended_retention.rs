use std::error::Error;

use automation_storage::AutomationStore;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn two_month_run_wake_and_revision_pruning_preserves_current_and_owed_work() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let mut observer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .foreign_keys(true),
    )
    .await?;
    let now_ms = chrono::DateTime::parse_from_rfc3339("2026-08-31T12:00:00Z")?.timestamp_millis();
    let cutoff_ms =
        chrono::DateTime::parse_from_rfc3339("2026-06-30T12:00:00Z")?.timestamp_millis();
    let old_ms = cutoff_ms - 1;
    let instruction_id = uuid::Uuid::now_v7().to_string();
    let current_revision_id = uuid::Uuid::now_v7().to_string();
    let old_revision_id = uuid::Uuid::now_v7().to_string();
    let boundary_revision_id = uuid::Uuid::now_v7().to_string();
    let schedule_id = uuid::Uuid::now_v7().to_string();
    let old_run_id = uuid::Uuid::now_v7().to_string();
    let boundary_run_id = uuid::Uuid::now_v7().to_string();
    let pending_run_id = uuid::Uuid::now_v7().to_string();
    let old_wake_id = uuid::Uuid::now_v7().to_string();
    let boundary_wake_id = uuid::Uuid::now_v7().to_string();
    let pending_wake_id = uuid::Uuid::now_v7().to_string();
    let remote_wake_id = uuid::Uuid::now_v7().to_string();
    let pending_delivery_id = uuid::Uuid::now_v7().to_string();
    let accepted_delivery_id = uuid::Uuid::now_v7().to_string();

    let mut transaction = observer.begin().await?;
    sqlx::query(
        "INSERT INTO instruction_documents(instruction_id,current_revision_id,instruction_text,updated_at_ms) VALUES (?,?,?,?)",
    )
    .bind(&instruction_id)
    .bind(&current_revision_id)
    .bind("current instruction")
    .bind(old_ms)
    .execute(&mut *transaction)
    .await?;
    for (revision_id, revision_text, recorded_at_ms) in [
        (old_revision_id.as_str(), "old instruction", old_ms),
        (
            boundary_revision_id.as_str(),
            "boundary instruction",
            cutoff_ms,
        ),
        (current_revision_id.as_str(), "current instruction", old_ms),
    ] {
        sqlx::query(
            "INSERT INTO instruction_revisions(revision_id,instruction_id,instruction_text,source_revision_id,recorded_at_ms) VALUES (?,?,?,NULL,?)",
        )
        .bind(revision_id)
        .bind(&instruction_id)
        .bind(revision_text)
        .bind(recorded_at_ms)
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query(
        "INSERT INTO schedule_definitions(schedule_id,change_id,instruction_id,enabled,definition_json,imported_continuity_json,created_at_ms,updated_at_ms) VALUES (?,?,?,0,'{}','{}',?,?)",
    )
    .bind(&schedule_id)
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(&instruction_id)
    .bind(old_ms)
    .bind(now_ms)
    .execute(&mut *transaction)
    .await?;
    for (run_id, run_status, completed_at_ms) in [
        (old_run_id.as_str(), "finished", Some(old_ms)),
        (
            boundary_run_id.as_str(),
            "preparationFailed",
            Some(cutoff_ms),
        ),
        (pending_run_id.as_str(), "uncertain", None),
    ] {
        sqlx::query(
            "INSERT INTO workflow_runs(run_id,schedule_id,due_at_ms,run_status,execution_evidence_json,completed_at_ms) VALUES (?,?,? ,?,'{}',?)",
        )
        .bind(run_id)
        .bind(&schedule_id)
        .bind(old_ms)
        .bind(run_status)
        .bind(completed_at_ms)
        .execute(&mut *transaction)
        .await?;
    }

    insert_wake(
        &mut transaction,
        &old_wake_id,
        "expired",
        old_ms,
        old_ms,
        Some(old_ms),
    )
    .await?;
    insert_wake(
        &mut transaction,
        &boundary_wake_id,
        "cancelled",
        old_ms,
        cutoff_ms,
        None,
    )
    .await?;
    insert_wake(
        &mut transaction,
        &pending_wake_id,
        "cancelled",
        old_ms,
        old_ms,
        None,
    )
    .await?;
    insert_wake(
        &mut transaction,
        &remote_wake_id,
        "expired",
        old_ms,
        old_ms,
        Some(old_ms),
    )
    .await?;
    insert_mailbox(
        &mut transaction,
        &pending_delivery_id,
        &pending_wake_id,
        "uncertain",
        old_ms,
    )
    .await?;
    sqlx::query("UPDATE wakeup_definitions SET pending_delivery_id=? WHERE wakeup_id=?")
        .bind(&pending_delivery_id)
        .bind(&pending_wake_id)
        .execute(&mut *transaction)
        .await?;
    insert_mailbox(
        &mut transaction,
        &accepted_delivery_id,
        &old_wake_id,
        "accepted",
        old_ms,
    )
    .await?;
    sqlx::query(
        "INSERT INTO remote_wake_requests(operation_id,target_json,canonical_request,body_json,valid_until_ms,wakeup_id,snapshot_json,created_at_ms) VALUES (?,'{}',x'01','{}',?,?,?,?)",
    )
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(old_ms)
    .bind(&remote_wake_id)
    .bind("{}")
    .bind(old_ms)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    observer.close().await?;

    let pruned_runs = store.prune_completed_workflow_runs(now_ms, 500).await?;
    if pruned_runs != 1 {
        return Err(format!("pruned {pruned_runs} completed runs; expected one").into());
    }
    let pruned_deliveries = store.prune_settled_mailbox_deliveries(now_ms, 500).await?;
    if pruned_deliveries != 1 {
        return Err(format!("pruned {pruned_deliveries} settled deliveries; expected one").into());
    }
    let pruned_wakes = store.prune_terminal_wakeups(now_ms, 500).await?;
    if pruned_wakes != 1 {
        return Err(format!("pruned {pruned_wakes} terminal wakes; expected one").into());
    }
    let pruned_revisions = store.prune_old_instruction_revisions(now_ms, 500).await?;
    if pruned_revisions != 1 {
        return Err(format!("pruned {pruned_revisions} old revisions; expected one").into());
    }

    let mut observer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .foreign_keys(true),
    )
    .await?;
    let run_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM workflow_runs WHERE run_id IN (?,?,?)")
            .bind(&old_run_id)
            .bind(&boundary_run_id)
            .bind(&pending_run_id)
            .fetch_one(&mut observer)
            .await?;
    if run_count != 2 {
        return Err(format!("retained {run_count} selected workflow runs; expected two").into());
    }
    let pending_wake_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM wakeup_definitions WHERE wakeup_id IN (?,?,?)")
            .bind(&boundary_wake_id)
            .bind(&pending_wake_id)
            .bind(&remote_wake_id)
            .fetch_one(&mut observer)
            .await?;
    if pending_wake_count != 3 {
        return Err(format!("retained {pending_wake_count} selected wakes; expected three").into());
    }
    let revision_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM instruction_revisions WHERE revision_id IN (?,?,?)",
    )
    .bind(&old_revision_id)
    .bind(&boundary_revision_id)
    .bind(&current_revision_id)
    .fetch_one(&mut observer)
    .await?;
    if revision_count != 2 {
        return Err(format!("retained {revision_count} selected revisions; expected two").into());
    }
    let remote_request_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM remote_wake_requests WHERE wakeup_id=?")
            .bind(&remote_wake_id)
            .fetch_one(&mut observer)
            .await?;
    if remote_request_count != 1 {
        return Err(
            format!("retained {remote_request_count} remote wake requests; expected one").into(),
        );
    }

    observer.close().await?;
    store.close().await?;
    Ok(())
}

async fn insert_wake(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    wake_id: &str,
    wake_status: &str,
    created_at_ms: i64,
    updated_at_ms: i64,
    expires_at_ms: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO wakeup_definitions(wakeup_id,change_id,wakeup_status,definition_json,anchor_at_ms,evaluated_through_ms,next_due_at_ms,expires_at_ms,first_fire_json,pending_delivery_id,created_at_ms,updated_at_ms) VALUES (?,?,?,'{}',?,?,NULL,?,NULL,NULL,?,?)",
    )
    .bind(wake_id)
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(wake_status)
    .bind(created_at_ms)
    .bind(created_at_ms)
    .bind(expires_at_ms)
    .bind(created_at_ms)
    .bind(updated_at_ms)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_mailbox(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    delivery_id: &str,
    wake_id: &str,
    status: &str,
    created_at_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO mailbox_deliveries(delivery_id,wakeup_id,occurrence_id,due_at_ms,fired_at_ms,target_json,message_json,delivery_mode,generation_guard_json,delivery_status,eligible_at_ms,expires_at_ms,latest_attempt_json,outcome_receipt_json,created_at_ms) VALUES (?,?,?,?,?,'{}','{}','queue',NULL,?,?,NULL,NULL,NULL,?)",
    )
    .bind(delivery_id)
    .bind(wake_id)
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(created_at_ms)
    .bind(created_at_ms)
    .bind(status)
    .bind(created_at_ms)
    .bind(created_at_ms)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
