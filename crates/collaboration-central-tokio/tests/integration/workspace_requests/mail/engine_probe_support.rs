//! Probe helpers shared by the engine V-check modules on the native host.
use super::super::{Statement, support::*};
use central_storage::{AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue};
use collaboration_central_core::{
    WorkspaceRuntime,
    engine_probe::{
        EngineRequestOutcome, InstanceKey, ProbeEvent, ProbeRefusal, ProbeRequest, ProbeState,
        SeedProbe,
    },
};
use collaboration_central_tokio::{ObjectStorage, TokioWorkspaceStores};
use std::sync::Arc;

pub(super) type Runtime = Arc<WorkspaceRuntime<ObjectStorage, TokioWorkspaceStores>>;
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
    fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap()
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
        sql: "SELECT (SELECT state FROM engine_probe_items WHERE item_id=?),(SELECT effect_count FROM engine_probe_groups WHERE group_id=?),(SELECT COUNT(*) FROM central_work_attempts WHERE machine_kind='engine_probe' AND subject_key=?)",
        args: vec![text(item), text(group), text(item)],
    }).await.unwrap();
    rows.rows.into_iter().next().unwrap()
}
pub(super) async fn attempt(runtime: &Runtime, item: uuid::Uuid) -> Vec<Vec<SqlValue>> {
    runtime.statement_host().read(Statement {
        sql: "SELECT state,failure_count,last_failure FROM central_work_attempts WHERE machine_kind='engine_probe' AND subject_key=?",
        args: vec![text(item)],
    }).await.unwrap().rows
}
pub(super) async fn receipts(runtime: &Runtime, request: u32) -> i64 {
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT COUNT(*) FROM request_log WHERE request_id=?",
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
pub(super) fn alarm_ms(runtime: &Runtime) -> Option<i64> {
    runtime
        .statement_host()
        .alarm()
        .map(|time| time.unix_millis())
}
