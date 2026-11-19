use central_engine_model::{ItemFailureClass, RetirementThreshold};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, Statement, StatementBatch,
    StoreFailure,
};
use fleet_contracts::{
    AlarmRecompute, ControlGate, DueWorkSource, EngineAttemptControl, EngineSubject,
    FailureOutcome, ObservedControl, SubjectVersion, WorkspaceDirectory, WorkspaceDrain,
};
use fleet_identity_protocol::OrganizationId;
use fleet_mail::CentralMail;
use fleet_runtime::{ClockIds, DueBatchState, FleetRuntime, engine_alarm_decider};
use fleet_runtime_ports::{CentralStore, SyncTime};
use fleet_runtime_test_support::{
    InMemoryCentralStore, InMemoryNoticeUnits, InMemoryWorkspaceStores, ManualClock,
};
use fleet_workspace_directory::CentralWorkspaceDirectory;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use workspace_model::{Description, ResourceName, Timestamp, WorkspaceId};

const WORKSPACE_ID: &str = "019f0000-0000-7000-8000-000000000001";

pub(super) async fn runtime()
-> Result<FleetRuntime<InMemoryCentralStore, InMemoryWorkspaceStores>, Box<dyn std::error::Error>> {
    let clock = ManualClock::starting_at(SyncTime::from_unix_millis(1_800_000_000_000));
    let due_batches = Arc::new(DueBatchState::default());
    let store = InMemoryCentralStore::open(
        &fleet_storage_schema::central_migrator(),
        Arc::new(clock.clone()),
        engine_alarm_decider(due_batches.clone()),
    )
    .await?;
    Ok(FleetRuntime::open(
        OrganizationId::try_from("019f0000-0000-7000-8000-000000000002".to_owned())?,
        Arc::new(store),
        InMemoryWorkspaceStores::default(),
        Arc::new(clock.clone()),
        Arc::new(ClockIds::new(clock)),
        due_batches,
        Arc::new(|_| {}),
    ))
}

#[tokio::test]
async fn directory_planning_leaves_sqlite_untouched_until_the_caller_applies_the_statement() {
    let runtime = runtime().await.unwrap();
    let directory = CentralWorkspaceDirectory::new(runtime.statement_host());
    let workspace: WorkspaceId = WORKSPACE_ID.parse().unwrap();
    let name = ResourceName::try_from("Research".to_owned()).unwrap();
    let description = Description::try_from("Shared notes".to_owned()).unwrap();

    let insert = WorkspaceDirectory::plan_guarded_insert(
        &directory,
        &workspace,
        &name,
        &description,
        runtime.now().unwrap(),
    )
    .unwrap();
    assert_eq!(insert.expect, RowExpectation::Exactly(1));
    assert!(
        WorkspaceDirectory::read_by_id(&directory, &workspace)
            .await
            .unwrap()
            .is_none()
    );
    let applied = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![insert],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(matches!(applied, BatchOutcome::Applied { .. }));

    let found = WorkspaceDirectory::read_by_name(&directory, &name)
        .await
        .unwrap()
        .unwrap();
    let view = WorkspaceDirectory::workspace_view(&directory, found).unwrap();
    assert_eq!(view.workspace_id.as_str(), workspace.as_str());
    assert_eq!(view.description.as_str(), description.as_str());
}

#[tokio::test]
async fn mail_persists_the_safety_mark_under_the_drain_contract() {
    let runtime = runtime().await.unwrap();
    let workspace: WorkspaceId = WORKSPACE_ID.parse().unwrap();
    let unexpected = Arc::new(AtomicUsize::new(0));
    let mail = CentralMail::new(&runtime, UnchangedAlarm(unexpected.clone()));
    let now = runtime.now().unwrap();
    let directory = CentralWorkspaceDirectory::new(runtime.statement_host());
    let insert = directory
        .plan_guarded_insert(
            &workspace,
            &ResourceName::try_from("Research".to_owned()).unwrap(),
            &Description::try_from(String::new()).unwrap(),
            now,
        )
        .unwrap();
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![insert],
                alarm: AlarmUpdate::Unchanged,
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));

    WorkspaceDrain::preset_workspace_drain(&mail, &workspace, now)
        .await
        .unwrap();

    let stored = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT drain_due_at FROM workspace_drains WHERE workspace_id=?",
            args: vec![fleet_runtime_ports::guarded_statements::text(&workspace)],
        })
        .await
        .unwrap();
    assert_eq!(
        stored.rows,
        vec![vec![central_storage::SqlValue::Text(
            (now + chrono::Duration::seconds(5))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        )]]
    );
    assert_eq!(
        DueWorkSource::plan_due_windows(&mail, now).await.unwrap(),
        (vec![], 0)
    );
    assert_eq!(unexpected.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn in_memory_notice_units_hold_pending_drains_until_the_safety_mark() {
    let units = InMemoryNoticeUnits::default();
    let workspace: WorkspaceId = WORKSPACE_ID.parse().unwrap();
    let now = Timestamp::from_timestamp_millis(1_800_000_000_000).unwrap();

    WorkspaceDrain::preset_workspace_drain(&units, &workspace, now)
        .await
        .unwrap();
    assert_eq!(
        DueWorkSource::drain_due_workspaces(&units, now)
            .await
            .unwrap(),
        0
    );

    let due = now + chrono::Duration::seconds(5);
    assert_eq!(units.scheduled(), vec![(workspace.clone(), due)]);
    assert_eq!(
        DueWorkSource::drain_due_workspaces(&units, due)
            .await
            .unwrap(),
        1
    );
    assert_eq!(units.drained(), vec![workspace]);
    assert!(units.scheduled().is_empty());
}

struct UnchangedAlarm(Arc<AtomicUsize>);

impl AlarmRecompute for UnchangedAlarm {
    fn alarm_update(&self) -> AlarmUpdate {
        AlarmUpdate::Unchanged
    }
}

impl EngineAttemptControl for UnchangedAlarm {
    async fn control_gate(&self, _: &EngineSubject, _: RetirementThreshold) -> ControlGate {
        ControlGate::Run(ObservedControl::Absent)
    }

    fn observe_engine_store_down(&self, _: &EngineSubject, _: StoreFailure) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    async fn record_failure_naming(
        &self,
        _: &EngineSubject,
        _: &SubjectVersion,
        _: ItemFailureClass,
        _: RetirementThreshold,
        _: &[WorkspaceId],
    ) -> FailureOutcome {
        self.0.fetch_add(1, Ordering::Relaxed);
        FailureOutcome::StoreDown(StoreFailure::Unavailable)
    }

    fn attempt_cleanup_statements(&self, _: &EngineSubject) -> GuardedStatement {
        self.0.fetch_add(1, Ordering::Relaxed);
        GuardedStatement {
            statement: Statement {
                sql: "SELECT 1",
                args: vec![],
            },
            expect: RowExpectation::Any,
        }
    }
}
