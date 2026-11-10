//! Retention read/delete failures back off independently while another class succeeds,
//! log one warning for the run, and recover with no new owner mutation.
use super::interaction_history_retention_tests::{history_value, load_broker};
use super::retention_test_harness::*;
use agent_automation::{InstructionText, OperationId};
use automation_storage::InstructionUpdate;
use chrono::{DateTime, SecondsFormat, Utc};
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, PushId};
use runtime_primitives::Clock;
use sqlx::Connection;
use std::{
    collections::HashMap,
    path::Path,
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
    InteractionRecords,
    InteractionHistory,
    InteractionHistoryRead,
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
            Self::InteractionRecords => "interaction records",
            Self::InteractionHistory | Self::InteractionHistoryRead => "interaction history",
        }
    }

    /// The table whose DELETE the fault makes fail, and the rows the class may delete.
    const fn table(self) -> &'static str {
        match self {
            Self::Events => "automation_events",
            Self::SettledMailboxDeliveries => "mailbox_deliveries",
            Self::OperationReceipts => "operation_receipts",
            Self::CompletedWorkflowRuns => "workflow_runs",
            Self::TerminalWakes => "wakeup_definitions",
            Self::OldInstructionRevisions => "instruction_revisions",
            Self::InteractionRecords => "router_pushes",
            Self::InteractionHistory | Self::InteractionHistoryRead => "typed_interaction_history",
        }
    }

    const fn in_interaction_store(self) -> bool {
        matches!(
            self,
            Self::InteractionRecords | Self::InteractionHistory | Self::InteractionHistoryRead
        )
    }
}

const AGED: &str = "2026-01-01T00:00:00.000Z";
const WALL: &str = "2026-06-15T12:00:00.000Z";

fn millis(text: &str) -> i64 {
    at(text).timestamp_millis()
}

async fn interaction_connection(directory: &Path) -> sqlx_turso::TursoConnection {
    sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new().filename(directory.join("interaction.sqlite")),
    )
    .await
    .unwrap()
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
    directory: &Path,
    store: &Arc<tokio::sync::Mutex<automation_storage::AutomationStore>>,
    writer: &mut sqlx::SqliteConnection,
) -> Option<Arc<interaction_storage::InteractionStore>> {
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
        FaultedClass::InteractionRecords => {
            let interactions = Arc::new(
                interaction_storage::InteractionStore::open(directory.join("interaction.sqlite"))
                    .await
                    .unwrap(),
            );
            let push = aging_push(at(AGED));
            let push_id: PushId = push.push_id.clone();
            interactions.insert_push_record(push).await.unwrap();
            interactions.mark_push_attempted(&push_id).await.unwrap();
            interactions
                .settle_push_record(
                    &push_id,
                    DeliveryReceipt {
                        outcome: DeliveryOutcome::PeerMessageWritten,
                        reachability: None,
                        client: None,
                    },
                    at(AGED),
                )
                .await
                .unwrap();
            return Some(interactions);
        }
        FaultedClass::InteractionHistory | FaultedClass::InteractionHistoryRead => {
            let mut history = interaction_connection(directory).await;
            let record = history_value("faulted-history", "aged refusal", None).unwrap();
            let mut transaction = history.begin().await.unwrap();
            sqlx::query("INSERT INTO typed_interaction_history (request_id,record_json,created_at) VALUES ('faulted-history',?,?)")
                .bind(serde_json::to_string(&record).unwrap())
                .bind(at(AGED).to_rfc3339_opts(SecondsFormat::Nanos, true))
                .execute(&mut *transaction)
                .await
                .unwrap();
            sqlx::query("UPDATE interaction_history_revision SET revision=revision+1")
                .execute(&mut *transaction)
                .await
                .unwrap();
            transaction.commit().await.unwrap();
            history.close().await.unwrap();
        }
    }
    None
}

async fn remaining_rows(
    class: FaultedClass,
    directory: &Path,
    writer: &mut sqlx::SqliteConnection,
) -> i64 {
    let count = match class {
        FaultedClass::Events => {
            "SELECT COUNT(*) FROM automation_events WHERE event_id='faulted-event'"
        }
        FaultedClass::OperationReceipts => {
            "SELECT COUNT(*) FROM operation_receipts WHERE operation_id='faulted-receipt'"
        }
        FaultedClass::SettledMailboxDeliveries => {
            "SELECT COUNT(*) FROM mailbox_deliveries WHERE delivery_id='faulted-delivery'"
        }
        FaultedClass::CompletedWorkflowRuns => {
            "SELECT COUNT(*) FROM workflow_runs WHERE run_id='older-run'"
        }
        FaultedClass::TerminalWakes => {
            "SELECT COUNT(*) FROM wakeup_definitions WHERE wakeup_id='cancelled-wake'"
        }
        FaultedClass::OldInstructionRevisions => "SELECT COUNT(*) FROM instruction_revisions",
        FaultedClass::InteractionRecords => "SELECT COUNT(*) FROM router_pushes",
        FaultedClass::InteractionHistory | FaultedClass::InteractionHistoryRead => {
            "SELECT COUNT(*) FROM typed_interaction_history WHERE request_id='faulted-history'"
        }
    };
    let remaining = if class.in_interaction_store() {
        let mut connection = interaction_connection(directory).await;
        let remaining = sqlx::query_scalar::<sqlx_turso::Turso, i64>(count)
            .fetch_one(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
        remaining
    } else {
        sqlx::query_scalar(count).fetch_one(writer).await.unwrap()
    };
    // The newest revision of an instruction always stays.
    if matches!(class, FaultedClass::OldInstructionRevisions) {
        remaining - 1
    } else {
        remaining
    }
}

async fn set_fault(
    class: FaultedClass,
    directory: &Path,
    writer: &mut sqlx::SqliteConnection,
    present: bool,
) {
    let statement = if matches!(class, FaultedClass::InteractionHistoryRead) {
        if present {
            "ALTER TABLE typed_interaction_history RENAME COLUMN record_json TO unavailable_record_json"
        } else {
            "ALTER TABLE typed_interaction_history RENAME COLUMN unavailable_record_json TO record_json"
        }
        .to_owned()
    } else if present {
        format!(
            "CREATE TRIGGER fail_class_prune BEFORE DELETE ON {} BEGIN SELECT RAISE(FAIL, 'class prune failure'); END",
            class.table()
        )
    } else {
        "DROP TRIGGER fail_class_prune".to_owned()
    };
    if class.in_interaction_store() {
        let mut connection = interaction_connection(directory).await;
        sqlx::raw_sql(sqlx::AssertSqlSafe(statement.clone()))
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
    } else {
        sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
            .execute(writer)
            .await
            .unwrap();
    }
}

async fn assert_class_backs_off_and_recovers(class: FaultedClass) {
    let counts = Arc::new(StdMutex::new(HashMap::new()));
    let _trace = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(RetentionLog(counts.clone())),
    );
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let mut writer = fixture_writer(directory.path()).await;
    let gate = crate::NativeGenerationGate::default();
    if matches!(
        class,
        FaultedClass::InteractionHistory | FaultedClass::InteractionHistoryRead
    ) {
        // Loading the broker once creates the history tables the fixture writes.
        drop(load_broker(directory.path(), &gate).await.unwrap());
    }
    let interactions = seed_aged_row(class, directory.path(), &store, &mut writer).await;
    let broker = if matches!(
        class,
        FaultedClass::InteractionHistory | FaultedClass::InteractionHistoryRead
    ) {
        Some(load_broker(directory.path(), &gate).await.unwrap())
    } else {
        None
    };
    // Another class with an aged row succeeds in the same passes.
    let other_succeeds_on = if matches!(class, FaultedClass::Events) {
        insert_receipt(&mut writer, "succeeding-receipt").await;
        "SELECT COUNT(*) FROM operation_receipts WHERE operation_id='succeeding-receipt'"
    } else {
        insert_event(&mut writer, "succeeding-event", at(AGED)).await;
        "SELECT COUNT(*) FROM automation_events WHERE event_id='succeeding-event'"
    };
    set_fault(class, directory.path(), &mut writer, true).await;
    let wall = at(WALL);

    let worker = RunningWorker::start_full(&store, interactions, broker, wall).await;

    let context = format!("{class:?}");
    let other_left: i64 = sqlx::query_scalar(other_succeeds_on)
        .fetch_one(&mut writer)
        .await
        .unwrap();
    assert_eq!(other_left, 0, "the other class went on: {context}");
    assert_eq!(
        remaining_rows(class, directory.path(), &mut writer).await,
        1,
        "{context}"
    );
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

    set_fault(class, directory.path(), &mut writer, false).await;
    run_to_the_armed_time(&worker).await;

    assert_eq!(
        remaining_rows(class, directory.path(), &mut writer).await,
        0,
        "the retry needed only its time: {context}"
    );
    assert_eq!(logged("retention prune recovered"), Some(1), "{context}");
    worker.stop().await;
}

#[tokio::test]
async fn a_history_inventory_read_failure_backs_off_without_holding_other_retention_classes() {
    assert_class_backs_off_and_recovers(FaultedClass::InteractionHistoryRead).await;
}

#[tokio::test]
async fn every_retention_class_backs_off_on_its_own_and_recovers_after_its_delete_fails() {
    for class in [
        FaultedClass::Events,
        FaultedClass::SettledMailboxDeliveries,
        FaultedClass::OperationReceipts,
        FaultedClass::CompletedWorkflowRuns,
        FaultedClass::TerminalWakes,
        FaultedClass::OldInstructionRevisions,
        FaultedClass::InteractionRecords,
        FaultedClass::InteractionHistory,
    ] {
        assert_class_backs_off_and_recovers(class).await;
    }
}
