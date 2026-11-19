//! Probe helpers shared by the engine V-check modules on the workerd host.
use super::super::{Statement, support::*, workerd_host::RemoteRuntime};
use central_storage::{AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue};
use collaboration_central_core::engine_probe::{
    EngineRequestOutcome, InstanceKey, ProbeEvent, ProbeRefusal, ProbeRequest, ProbeState,
    SeedProbe,
};
use std::sync::{Arc, atomic::Ordering};

pub(super) type Runtime = Arc<RemoteRuntime>;
pub(super) type ProbeAnswer = EngineRequestOutcome<ProbeState, ProbeRefusal>;

pub(super) fn probe_id(number: u32) -> uuid::Uuid {
    format!("01890f2e-7b4c-7cc0-98c4-{number:012}")
        .parse()
        .unwrap()
}
pub(super) fn key(item: uuid::Uuid) -> InstanceKey {
    InstanceKey(item.to_string())
}
pub(super) async fn runtime(fixture: &Fixture) -> Runtime {
    fixture.host.central(&fixture.caller.organization_id).await
}
pub(super) fn now(fixture: &Fixture) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp_millis(fixture.clock.0.load(Ordering::SeqCst)).unwrap()
}
pub(super) fn set_now(fixture: &Fixture, at: chrono::DateTime<chrono::Utc>) {
    fixture
        .clock
        .0
        .store(at.timestamp_millis(), Ordering::SeqCst);
}
pub(super) async fn seed_probe(
    runtime: &Runtime,
    item: uuid::Uuid,
    group: uuid::Uuid,
    due: Option<chrono::DateTime<chrono::Utc>>,
    claim: &str,
) {
    assert!(matches!(
        runtime
            .seed_probe(SeedProbe {
                key: item,
                group,
                due,
                claim: claim.into(),
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
}
pub(super) fn probe_request(request: u32, item: uuid::Uuid, event: ProbeEvent) -> ProbeRequest {
    ProbeRequest {
        request_id: id(request),
        key: key(item),
        event,
    }
}
/// (item state, group effect count, attempt rows for the item)
pub(super) async fn probe_facts(
    runtime: &Runtime,
    item: uuid::Uuid,
    group: uuid::Uuid,
) -> Vec<SqlValue> {
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT (SELECT state FROM engine_probe_items WHERE item_id=?),(SELECT effect_count FROM engine_probe_groups WHERE group_id=?),(SELECT COUNT(*) FROM engine_work_attempts WHERE machine_kind='engine_probe' AND subject_key=?)",
        args: vec![text(item), text(group), text(item)],
    }).await.unwrap();
    rows.rows.into_iter().next().unwrap()
}
pub(super) async fn attempt(runtime: &Runtime, item: uuid::Uuid) -> Vec<Vec<SqlValue>> {
    runtime.statement_host().read(Statement {
        sql: "SELECT state,failure_count,last_failure FROM engine_work_attempts WHERE machine_kind='engine_probe' AND subject_key=?",
        args: vec![text(item)],
    }).await.unwrap().rows
}
pub(super) async fn receipts(runtime: &Runtime, request: u32) -> i64 {
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT COUNT(*) FROM runtime_request_log WHERE request_id=?",
            args: vec![text(id::<workspace_model::RequestId>(request))],
        })
        .await
        .unwrap();
    match rows.rows[0][0] {
        SqlValue::Integer(count) => count.get(),
        ref other => panic!("count: {other:?}"),
    }
}
pub(super) fn integer(value: i64) -> SqlValue {
    SqlValue::Integer(value.try_into().unwrap())
}
/// Runs one raw statement, for corruptions and test triggers only.
pub(super) async fn raw(runtime: &Runtime, sql: &'static str, args: Vec<SqlValue>) {
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(central_storage::StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement { sql, args },
                    expect: RowExpectation::Any,
                }],
                alarm: AlarmUpdate::Unchanged,
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
}
pub(super) async fn alarm_ms(runtime: &Runtime) -> Option<i64> {
    runtime
        .control::<Option<i64>>("raw-alarm-time", &())
        .await
        .unwrap()
}

/// The workerd twin of `PausePoints::hold_engine`, driven through test routes.
pub(super) struct RemotePauses(Runtime);
pub(super) struct RemoteHold {
    runtime: Runtime,
    id: u64,
}
pub(super) trait PauseControl {
    fn pause_points(&self) -> RemotePauses;
}
impl PauseControl for Runtime {
    fn pause_points(&self) -> RemotePauses {
        RemotePauses(self.clone())
    }
}
impl RemotePauses {
    pub(super) async fn hold_engine(
        &self,
        point: collaboration_central_core::PausePoint,
        key: &str,
    ) -> RemoteHold {
        use collaboration_central_core::PausePoint as P;
        let point = match point {
            P::AfterEngineAttemptGate => "afterEngineAttemptGate",
            P::AfterEngineLoad => "afterEngineLoad",
            P::AfterEngineCommit => "afterEngineCommit",
            P::BeforeEngineFailureWrite => "beforeEngineFailureWrite",
            P::AfterEngineAbsentLookup => "afterEngineAbsentLookup",
            other => panic!("not an engine point: {other:?}"),
        };
        RemoteHold {
            runtime: self.0.clone(),
            id: self.0.hold_engine(point, key).await,
        }
    }
}
impl RemoteHold {
    pub(super) async fn until_held(&mut self) {
        self.runtime.until_held_engine(self.id).await;
    }
    pub(super) async fn release(self) {
        self.runtime.release_engine(self.id).await;
    }
}
