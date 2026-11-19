use super::workspace_contracts::runtime;
use central_engine_model::{
    EngineTime, InstanceKey, ItemFailureClass, MachineKind, RetirementThreshold,
};
use central_storage::{
    AlarmPublication, BatchOutcome, GuardedStatement, StatementBatch, StoreFailure,
};
use fleet_contracts::{
    AlarmRecompute, CentralObservation, ControlGate, DueWorkSource, EngineAttemptControl,
    EngineSubject, FailureOutcome, LegacyUnitOutcome, ObservedControl, OrganizationStamp,
    SubjectVersion,
};
use fleet_engine::{Engine, EngineHost};
use fleet_runtime::{DueBatchState, FleetRuntime, LegacyUnitInvocations, PausePoints, TurnGuard};
use fleet_runtime_ports::{CentralStore, IdSource};
use fleet_runtime_test_support::{
    InMemoryCentralStore, InMemoryNoticeUnits, InMemoryWorkspaceStores,
};

struct InMemoryEngineHost<'a> {
    runtime: &'a FleetRuntime<InMemoryCentralStore, InMemoryWorkspaceStores>,
    notices: InMemoryNoticeUnits,
}

impl OrganizationStamp for InMemoryEngineHost<'_> {
    async fn organization_is_provisioned(&self) -> Result<bool, StoreFailure> {
        Ok(false)
    }
}

impl EngineHost for InMemoryEngineHost<'_> {
    type Host = InMemoryCentralStore;

    fn host(&self) -> &Self::Host {
        self.runtime.statement_host()
    }

    fn now(&self) -> Result<EngineTime, StoreFailure> {
        self.runtime.now()
    }

    fn ids(&self) -> &dyn IdSource {
        self.runtime.ids()
    }

    fn due_batches(&self) -> &DueBatchState {
        self.runtime.due_batches()
    }

    fn observe(&self, observation: CentralObservation) {
        self.runtime.observe(observation);
    }

    async fn organization_is_provisioned(&self) -> Result<bool, StoreFailure> {
        OrganizationStamp::organization_is_provisioned(self).await
    }

    async fn plan_due_windows(
        &self,
        now: EngineTime,
    ) -> Result<(Vec<GuardedStatement>, usize), LegacyUnitOutcome> {
        self.notices.plan_due_windows(now).await
    }

    async fn drain_due_workspaces(&self, now: EngineTime) -> Result<usize, LegacyUnitOutcome> {
        self.notices.drain_due_workspaces(now).await
    }

    fn try_central_turn(&self) -> Option<TurnGuard<'_>> {
        self.runtime.try_central_turn()
    }

    fn pauses(&self) -> &PausePoints {
        self.runtime.pause_points()
    }

    fn count_legacy_invocation(&self, count: &dyn Fn(&mut LegacyUnitInvocations)) {
        self.runtime.count_legacy_invocation(count);
    }
}

#[tokio::test]
async fn engine_contracts_record_gate_and_clear_an_attempt_through_the_store() {
    let runtime = runtime().await.unwrap();
    let host = InMemoryEngineHost {
        runtime: &runtime,
        notices: InMemoryNoticeUnits::default(),
    };
    let engine = Engine::new(&host);
    let subject = EngineSubject {
        kind: MachineKind("legacy_workspace_drains"),
        key: InstanceKey("019f0000-0000-7000-8000-000000000001".to_owned()),
    };
    let threshold = RetirementThreshold::NOTICE_UNITS;

    let failure = EngineAttemptControl::record_failure_naming(
        &engine,
        &subject,
        &SubjectVersion(vec![]),
        ItemFailureClass::OtherStoreUnavailable,
        threshold,
        &[],
    )
    .await;

    assert!(matches!(failure, FailureOutcome::Recorded(_)));
    assert!(matches!(
        EngineAttemptControl::control_gate(&engine, &subject, threshold).await,
        ControlGate::Skip
    ));
    let cleanup = EngineAttemptControl::attempt_cleanup_statements(&engine, &subject);
    let outcome = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![cleanup],
            alarm: AlarmRecompute::alarm_update(&engine),
        })
        .await
        .unwrap();
    assert_eq!(
        outcome,
        BatchOutcome::Applied {
            rows_affected: vec![1],
            publication: AlarmPublication::Cleared,
        }
    );
    assert!(matches!(
        EngineAttemptControl::control_gate(&engine, &subject, threshold).await,
        ControlGate::Run(ObservedControl::Absent)
    ));
}
