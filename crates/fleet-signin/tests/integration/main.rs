use central_storage::{AlarmPublication, AlarmUpdate, BatchOutcome, Statement, StatementBatch};
use fleet_contracts::{AlarmRecompute, OrganizationStamp};
use fleet_identity_protocol::OrganizationId;
use fleet_runtime::{ClockIds, DueBatchState, FleetRuntime, engine_alarm_decider};
use fleet_runtime_ports::{CentralStore, SyncTime};
use fleet_runtime_test_support::{InMemoryCentralStore, InMemoryWorkspaceStores, ManualClock};
use fleet_signin::{OrganizationAuthority, OrganizationSignIn};
use std::sync::Arc;

struct EmptyAlarmQuery;

impl AlarmRecompute for EmptyAlarmQuery {
    fn alarm_update(&self) -> AlarmUpdate {
        AlarmUpdate::SetFromQuery(Statement {
            sql: "SELECT NULL",
            args: vec![],
        })
    }
}

#[tokio::test]
async fn reading_an_unprovisioned_stamp_clears_the_published_alarm() {
    let clock = ManualClock::starting_at(SyncTime::from_unix_millis(1_800_000_000_000));
    let due_batches = Arc::new(DueBatchState::default());
    let store = InMemoryCentralStore::open(
        &fleet_storage_schema::central_migrator(),
        Arc::new(clock.clone()),
        engine_alarm_decider(due_batches.clone()),
    )
    .await
    .unwrap();
    let runtime = FleetRuntime::open(
        OrganizationId::try_from("019f0000-0000-7000-8000-000000000002".to_owned()).unwrap(),
        Arc::new(store),
        InMemoryWorkspaceStores::default(),
        Arc::new(clock.clone()),
        Arc::new(ClockIds::new(clock)),
        due_batches,
        Arc::new(|_| {}),
    );
    let applied = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![],
            alarm: AlarmUpdate::SetFromQuery(Statement {
                sql: "SELECT 1800000000500",
                args: vec![],
            }),
        })
        .await
        .unwrap();
    assert_eq!(
        applied,
        BatchOutcome::Applied {
            rows_affected: vec![],
            publication: AlarmPublication::Set {
                unix_millis: 1_800_000_000_500
            },
        }
    );
    assert!(
        runtime
            .statement_host()
            .published_alarm()
            .await
            .unwrap()
            .is_some()
    );

    let authority = OrganizationAuthority::open(Ok(None)).unwrap();
    let sign_in = OrganizationSignIn::new(&runtime, &authority, EmptyAlarmQuery);
    assert!(
        !OrganizationStamp::organization_is_provisioned(&sign_in)
            .await
            .unwrap()
    );
    assert_eq!(
        runtime.statement_host().published_alarm().await.unwrap(),
        None
    );
}
