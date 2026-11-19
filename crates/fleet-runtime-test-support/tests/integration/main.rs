use central_storage::{AlarmPublication, AlarmUpdate, BatchOutcome, Statement, StatementBatch};
use fleet_identity_protocol::OrganizationId;
use fleet_runtime::{ClockIds, DueBatchState, FleetRuntime, engine_alarm_decider};
use fleet_runtime_ports::{CentralStore, EngineTime, SyncTime};
use fleet_runtime_test_support::{InMemoryCentralStore, InMemoryWorkspaceStores, ManualClock};
use sqlx::migrate::Migrator;
use std::sync::Arc;

mod engine_contracts;
mod workspace_contracts;

#[tokio::test]
async fn a_real_sqlite_alarm_query_publishes_the_mark_through_the_runtime_store() {
    let now_ms = 1_800_000_000_000;
    let due_ms = now_ms + 300_000;
    let clock = ManualClock::starting_at(SyncTime::from_unix_millis(now_ms));
    let due_batches = Arc::new(DueBatchState::default());
    let store = InMemoryCentralStore::open(
        &Migrator::with_migrations(Vec::new()),
        Arc::new(clock.clone()),
        engine_alarm_decider(due_batches.clone()),
    )
    .await
    .unwrap();
    let runtime = FleetRuntime::open(
        OrganizationId::try_from("019f0000-0000-7000-8000-000000000001".to_owned()).unwrap(),
        Arc::new(store),
        InMemoryWorkspaceStores::default(),
        Arc::new(clock.clone()),
        Arc::new(ClockIds::new(clock)),
        due_batches,
        Arc::new(|_| {}),
    );

    let batch = StatementBatch {
        statements: Vec::new(),
        alarm: AlarmUpdate::SetEngineFromQuery {
            query: Statement {
                sql: "SELECT ?, NULL",
                args: vec![central_storage::SqlValue::Integer(
                    central_storage::SafeInteger::new(due_ms).unwrap(),
                )],
            },
        },
    };
    let receipt = runtime.statement_host().run_batch(batch).await.unwrap();

    assert_eq!(
        receipt,
        BatchOutcome::Applied {
            rows_affected: Vec::new(),
            publication: AlarmPublication::Set {
                unix_millis: due_ms
            },
        }
    );
    assert_eq!(
        runtime.statement_host().published_alarm().await.unwrap(),
        EngineTime::from_timestamp_millis(due_ms)
    );
}
