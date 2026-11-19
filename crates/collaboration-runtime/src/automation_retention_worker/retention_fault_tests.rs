//! Automation retention read/delete failures back off independently while another class
//! succeeds, log one warning for the run, and recover with no new owner mutation.
use super::retention_test_harness::*;
use agent_automation::{InstructionText, OperationId};
use automation_storage::InstructionUpdate;
use chrono::{DateTime, Utc};
use runtime_primitives::Clock;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};
use tracing_subscriber::layer::SubscriberExt;

#[derive(Clone, Copy, Debug)]
enum FaultedClass {
    Events,
    SettledMailboxDeliveries,
    OperationReceipts,
    CompletedWorkflowRuns,
    TerminalWakes,
    OldInstructionRevisions,
}

impl FaultedClass {
    /// The name the worker logs the class under.
    const fn logged_name(self) -> &'static str {
        match self {
            Self::Events => "automation events",
            Self::SettledMailboxDeliveries => "settled mailbox deliveries",
            Self::OperationReceipts => "operation receipts",
            Self::CompletedWorkflowRuns => "completed workflow runs",
            Self::TerminalWakes => "expired or cancelled wakes",
            Self::OldInstructionRevisions => "old instruction revisions",
        }
    }

    /// The table whose DELETE the fault makes fail.
    const fn table(self) -> &'static str {
        match self {
            Self::Events => "automation_events",
            Self::SettledMailboxDeliveries => "mailbox_deliveries",
            Self::OperationReceipts => "operation_receipts",
            Self::CompletedWorkflowRuns => "workflow_runs",
            Self::TerminalWakes => "wakeup_definitions",
            Self::OldInstructionRevisions => "instruction_revisions",
        }
    }

    /// Counts the rows the class may delete.
    const fn remaining(self) -> &'static str {
        match self {
            Self::Events => "SELECT COUNT(*) FROM automation_events WHERE event_id='faulted-event'",
            Self::OperationReceipts => {
                "SELECT COUNT(*) FROM operation_receipts WHERE operation_id='faulted-receipt'"
            }
            Self::SettledMailboxDeliveries => {
                "SELECT COUNT(*) FROM mailbox_deliveries WHERE delivery_id='faulted-delivery'"
            }
            Self::CompletedWorkflowRuns => {
                "SELECT COUNT(*) FROM workflow_runs WHERE run_id='older-run'"
            }
            Self::TerminalWakes => {
                "SELECT COUNT(*) FROM wakeup_definitions WHERE wakeup_id='cancelled-wake'"
            }
            // The newest revision of an instruction always stays.
            Self::OldInstructionRevisions => "SELECT COUNT(*) - 1 FROM instruction_revisions",
        }
    }
}

const AGED: &str = "2026-01-01T00:00:00.000Z";
const WALL: &str = "2026-06-15T12:00:00.000Z";

fn millis(text: &str) -> i64 {
    at(text).timestamp_millis()
}

async fn insert_receipt(writer: &mut sqlx::SqliteConnection, id: &str) {
    sqlx::query("INSERT INTO operation_receipts(operation_id,method_name,canonical_request,resource_id,operation_status,effect_evidence_json,final_result_json,committed_at_ms) VALUES (?,'fixture',x'00','fixture','succeeded','[]','{}',?)")
        .bind(id)
        .bind(millis(AGED))
        .execute(writer)
        .await
        .unwrap();
}

async fn insert_wake(writer: &mut sqlx::SqliteConnection, id: &str, status: &str) {
    sqlx::query("INSERT INTO wakeup_definitions (wakeup_id,change_id,wakeup_status,definition_json,anchor_at_ms,evaluated_through_ms,created_at_ms,updated_at_ms) VALUES (?,?,?,'{}',?,?,?,?)")
        .bind(id)
        .bind(format!("{id}-change"))
        .bind(status)
        .bind(millis(AGED))
        .bind(millis(AGED))
        .bind(millis(AGED))
        .bind(millis(AGED))
        .execute(writer)
        .await
        .unwrap();
}

/// Writes an aged row the class may delete, so its DELETE is due on the first pass.
async fn seed_aged_row(
    class: FaultedClass,
    store: &Arc<tokio::sync::Mutex<automation_storage::AutomationStore>>,
    writer: &mut sqlx::SqliteConnection,
) {
    match class {
        FaultedClass::Events => insert_event(writer, "faulted-event", at(AGED)).await,
        FaultedClass::OperationReceipts => insert_receipt(writer, "faulted-receipt").await,
        FaultedClass::SettledMailboxDeliveries => {
            insert_wake(writer, "finished-wake", "finished").await;
            sqlx::query("INSERT INTO mailbox_deliveries (delivery_id,wakeup_id,occurrence_id,due_at_ms,fired_at_ms,target_json,message_json,delivery_mode,delivery_status,eligible_at_ms,created_at_ms) VALUES ('faulted-delivery','finished-wake','occurrence',?,?,'{}','{}','queue','accepted',?,?)")
                .bind(millis(AGED))
                .bind(millis(AGED))
                .bind(millis(AGED))
                .bind(millis(AGED))
                .execute(&mut *writer)
                .await
                .unwrap();
        }
        FaultedClass::CompletedWorkflowRuns => {
            // The schedule's newest finished run stays; the older one may go.
            for (run, completed) in [("older-run", millis(AGED)), ("newest-run", millis(WALL))] {
                sqlx::query("INSERT INTO workflow_runs (run_id,schedule_id,due_at_ms,run_status,execution_evidence_json,completed_at_ms) VALUES (?,'schedule',0,'finished','{}',?)")
                    .bind(run)
                    .bind(completed)
                    .execute(&mut *writer)
                    .await
                    .unwrap();
            }
        }
        FaultedClass::TerminalWakes => insert_wake(writer, "cancelled-wake", "cancelled").await,
        FaultedClass::OldInstructionRevisions => {
            let instruction = store
                .lock()
                .await
                .create_instruction(
                    &OperationId::generate(),
                    &InstructionText::try_from("first revision".to_owned()).unwrap(),
                    millis(AGED),
                )
                .await
                .unwrap();
            store
                .lock()
                .await
                .update_instruction(&InstructionUpdate {
                    operation_id: OperationId::generate(),
                    instruction_id: instruction.instruction_id,
                    expected_revision_id: instruction.revision_id,
                    text: InstructionText::try_from("second revision".to_owned()).unwrap(),
                    now_ms: millis(WALL),
                })
                .await
                .unwrap();
        }
    }
}

async fn set_fault(class: FaultedClass, writer: &mut sqlx::SqliteConnection, present: bool) {
    let statement = if present {
        format!(
            "CREATE TRIGGER fail_class_prune BEFORE DELETE ON {} BEGIN SELECT RAISE(FAIL, 'class prune failure'); END",
            class.table()
        )
    } else {
        "DROP TRIGGER fail_class_prune".to_owned()
    };
    sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
        .execute(writer)
        .await
        .unwrap();
}

async fn assert_class_backs_off_and_recovers(class: FaultedClass) {
    let counts = Arc::new(StdMutex::new(HashMap::new()));
    let _trace = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(RetentionLog(counts.clone())),
    );
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let mut writer = fixture_writer(directory.path()).await;
    seed_aged_row(class, &store, &mut writer).await;
    // Another class with an aged row succeeds in the same passes.
    let other_succeeds_on = if matches!(class, FaultedClass::Events) {
        insert_receipt(&mut writer, "succeeding-receipt").await;
        "SELECT COUNT(*) FROM operation_receipts WHERE operation_id='succeeding-receipt'"
    } else {
        insert_event(&mut writer, "succeeding-event", at(AGED)).await;
        "SELECT COUNT(*) FROM automation_events WHERE event_id='succeeding-event'"
    };
    set_fault(class, &mut writer, true).await;
    let wall = at(WALL);

    let worker = start(&store, wall).await;

    let context = format!("{class:?}");
    let other_left: i64 = sqlx::query_scalar(other_succeeds_on)
        .fetch_one(&mut writer)
        .await
        .unwrap();
    assert_eq!(other_left, 0, "the other class went on: {context}");
    let remaining: i64 = sqlx::query_scalar(class.remaining())
        .fetch_one(&mut writer)
        .await
        .unwrap();
    assert_eq!(remaining, 1, "{context}");
    let first_retry = armed(&worker.snapshot()).unwrap();
    let parked_wall =
        DateTime::<Utc>::from_timestamp_millis(worker.clock.now().unix_millis()).unwrap();
    assert!(
        first_retry >= wall + chrono::Duration::seconds(1)
            && first_retry <= parked_wall + chrono::Duration::seconds(1),
        "the first retry is one second after the failure: {context} {first_retry}"
    );
    let mut previous = first_retry;
    for expected in [2_000, 4_000] {
        run_to_the_armed_time(&worker).await;
        let next = armed(&worker.snapshot()).unwrap();
        let step = (next - previous).num_milliseconds();
        // The manual clock reaches each retry's exact deadline.
        assert!(
            (expected..=expected + 2).contains(&step),
            "{context}: step {step}, expected {expected}"
        );
        previous = next;
    }
    let logged = |message: &str| {
        counts
            .lock()
            .unwrap()
            .get(&format!("{}: {message}", class.logged_name()))
            .copied()
    };
    assert_eq!(
        logged("retention prune failed; it retries with backoff"),
        Some(1),
        "{context}"
    );

    set_fault(class, &mut writer, false).await;
    run_to_the_armed_time(&worker).await;

    let remaining: i64 = sqlx::query_scalar(class.remaining())
        .fetch_one(&mut writer)
        .await
        .unwrap();
    assert_eq!(remaining, 0, "the retry needed only its time: {context}");
    assert_eq!(logged("retention prune recovered"), Some(1), "{context}");
    worker.stop().await;
}

#[tokio::test]
async fn every_automation_retention_class_backs_off_on_its_own_and_recovers_after_its_delete_fails()
{
    for class in [
        FaultedClass::Events,
        FaultedClass::SettledMailboxDeliveries,
        FaultedClass::OperationReceipts,
        FaultedClass::CompletedWorkflowRuns,
        FaultedClass::TerminalWakes,
        FaultedClass::OldInstructionRevisions,
    ] {
        assert_class_backs_off_and_recovers(class).await;
    }
}
