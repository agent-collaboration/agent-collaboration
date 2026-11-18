//! V2 error origin on the workerd host: each drain-unit outcome forced
//! separately, then mixed across two workspaces. The plain workerd fixture
//! captures no Worker logs, so the observation naming the workspace is
//! asserted on the native host only.
use super::super::engine_evidence::{
    assert_fixture_attempt_ids_are_uuid_v7, witness_fixture_attempt_ids,
};
use super::super::engine_v2_support::{
    alarm_ms, next_mark, now, raw, record, rows, runtime, set_now, set_wake, stamp, wake_messages,
};
use super::*;
use collaboration_central_core::engine_probe::InstanceKey;
use fleet_runtime::{EngineCommitFault, EngineCommitFaultKind};

const DRAINS: &str = "legacy_workspace_drains";

async fn second_workspace(f: &Fixture, request: u32) -> m::WorkspaceId {
    let reply = call(
        &f.host,
        &f.caller,
        None,
        request,
        m::WorkspaceOperation::CreateWorkspace {
            name: name(&format!("Origin workspace {request}")),
        },
    )
    .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace }) = reply.result
    else {
        panic!("second workspace: {reply:?}")
    };
    workspace.workspace_id
}
async fn drain_due_at(f: &Fixture, workspace: &m::WorkspaceId, due: chrono::DateTime<chrono::Utc>) {
    raw(
        &runtime(f).await,
        "INSERT INTO workspace_drains(workspace_id,drained_through_position,drain_due_at,created_at,updated_at) VALUES (?,0,?,?,?) ON CONFLICT(workspace_id) DO UPDATE SET drain_due_at=excluded.drain_due_at",
        vec![text(workspace), stamp(due), stamp(due), stamp(due)],
    )
    .await;
}
async fn drain_row(f: &Fixture, workspace: &m::WorkspaceId) -> Vec<SqlValue> {
    rows(
        &runtime(f).await,
        "SELECT drained_through_position,drain_due_at FROM workspace_drains WHERE workspace_id=?",
        vec![text(workspace)],
    )
    .await
    .remove(0)
}
fn leak(sql: String) -> &'static str {
    Box::leak(sql.into_boxed_str())
}
/// A real unique index the drain's own cursor update violates for the listed
/// workspaces: an idle third workspace's row already holds the `updated_at`
/// the drain writes.
async fn collide_drain_updates(
    f: &Fixture,
    request: u32,
    workspaces: &[&m::WorkspaceId],
    at: chrono::DateTime<chrono::Utc>,
) {
    let holder = second_workspace(f, request).await;
    let runtime = runtime(f).await;
    raw(
        &runtime,
        "INSERT INTO workspace_drains(workspace_id,drained_through_position,drain_due_at,created_at,updated_at) VALUES (?,0,NULL,?,?) ON CONFLICT(workspace_id) DO UPDATE SET drain_due_at=NULL,updated_at=excluded.updated_at",
        vec![text(&holder), stamp(at), stamp(at)],
    )
    .await;
    let mut members: Vec<String> = workspaces.iter().map(|w| format!("'{w}'")).collect();
    members.push(format!("'{holder}'"));
    raw(
        &runtime,
        leak(format!(
            "CREATE UNIQUE INDEX drain_updated_once ON workspace_drains(updated_at) WHERE workspace_id IN ({})",
            members.join(",")
        )),
        vec![],
    )
    .await;
}
struct Case {
    fixture: Fixture,
    wake: String,
    at: chrono::DateTime<chrono::Utc>,
}
async fn case() -> Case {
    let fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    fixture.area(8600).await;
    let at = next_mark(now(&fixture));
    drain_due_at(
        &fixture,
        &fixture.workspace,
        at - chrono::Duration::seconds(1),
    )
    .await;
    let wake = set_wake(&fixture, 8601, at, "batch wake").await;
    Case { fixture, wake, at }
}
async fn run(case: &Case) {
    set_now(&case.fixture, case.at);
    runtime(&case.fixture).await.after_event().await.unwrap();
}
async fn fault(f: &Fixture, workspace: &m::WorkspaceId, kind: EngineCommitFaultKind) {
    runtime(f)
        .await
        .inject_engine_commit_fault(EngineCommitFault {
            key: InstanceKey(workspace.to_string()),
            kind,
        })
        .await;
}

#[tokio::test]
async fn a_workspace_store_outage_is_recorded_and_the_batch_continues() {
    let case = case().await;
    let f = &case.fixture;
    f.workspace_execute(
        "ALTER TABLE changes RENAME TO outage_changes",
        vec![],
        central_storage::RowExpectation::Any,
    )
    .await;

    run(&case).await;

    let runtime = runtime(f).await;
    assert_eq!(
        wake_messages(&runtime, &case.wake).await,
        1,
        "the rest of the batch continues"
    );
    assert_eq!(
        record(&runtime, DRAINS, "all").await[0][..4],
        [
            text("failing"),
            SqlValue::Integer(1.try_into().unwrap()),
            stamp(case.at + chrono::Duration::minutes(5)),
            text("other_store_unavailable")
        ]
    );
    assert_fixture_attempt_ids_are_uuid_v7(
        f,
        "a_workspace_store_outage_is_recorded_and_the_batch_continues",
        &[],
    )
    .await;
    f.host.close().await;
}

#[tokio::test]
async fn central_down_on_the_drain_batch_records_nothing_and_rearms() {
    let case = case().await;
    let f = &case.fixture;
    let before = drain_row(f, &f.workspace).await;
    fault(f, &f.workspace, EngineCommitFaultKind::Unavailable).await;

    run(&case).await;

    let runtime = runtime(f).await;
    assert!(
        !runtime.engine_commit_fault_pending().await,
        "the drain batch met the fault"
    );
    assert!(
        record(&runtime, DRAINS, "all").await.is_empty(),
        "CentralDown: no record"
    );
    assert_eq!(drain_row(f, &f.workspace).await, before, "nothing drained");
    let rearm = alarm_ms(&runtime)
        .await
        .expect("the re-arm is attempted and published");
    assert!(rearm > case.at.timestamp_millis());
    set_now(f, chrono::DateTime::from_timestamp_millis(rearm).unwrap());
    runtime.after_event().await.unwrap();
    assert_eq!(
        drain_row(f, &f.workspace).await[1],
        SqlValue::Null,
        "drained once central is back"
    );
    assert_fixture_attempt_ids_are_uuid_v7(
        f,
        "central_down_on_the_drain_batch_records_nothing_and_rearms",
        &[],
    )
    .await;
    f.host.close().await;
}

#[tokio::test]
async fn rejected_and_uncertain_and_guard_failed_drain_batches_each_take_their_own_outcome() {
    for (kind, expected) in [
        (
            EngineCommitFaultKind::DatabaseCode(19),
            Some("statement_rejected"),
        ),
        (EngineCommitFaultKind::NotAppliedAnswerLost, None),
        (EngineCommitFaultKind::GuardFailed, None),
    ] {
        let case = case().await;
        let f = &case.fixture;
        let before = drain_row(f, &f.workspace).await;
        fault(f, &f.workspace, kind.clone()).await;

        run(&case).await;

        let runtime = runtime(f).await;
        assert!(
            !runtime.engine_commit_fault_pending().await,
            "{kind:?}: met"
        );
        assert_eq!(wake_messages(&runtime, &case.wake).await, 1, "{kind:?}");
        let found = record(&runtime, DRAINS, "all").await;
        match expected {
            Some(class) => {
                assert_eq!(
                    found[0][..2],
                    [text("failing"), SqlValue::Integer(1.try_into().unwrap())],
                    "{kind:?}"
                );
                assert_eq!(found[0][3], text(class), "{kind:?}");
            }
            None => assert!(found.is_empty(), "{kind:?}: not a failure, no record"),
        }
        assert_eq!(
            drain_row(f, &f.workspace).await,
            before,
            "{kind:?}: nothing committed"
        );
        assert_fixture_attempt_ids_are_uuid_v7(
            f,
            "rejected_and_uncertain_and_guard_failed_drain_batches_each_take_their_own_outcome",
            &[],
        )
        .await;
        f.host.close().await;
    }
}

#[tokio::test]
async fn a_unique_constraint_on_the_drain_batch_is_recorded_as_constraint_violated() {
    let case = case().await;
    let f = &case.fixture;
    collide_drain_updates(f, 8630, &[&f.workspace], case.at).await;
    let before = drain_row(f, &f.workspace).await;

    run(&case).await;

    let runtime = runtime(f).await;
    let found = record(&runtime, DRAINS, "all").await;
    assert_eq!(
        found[0][..2],
        [text("failing"), SqlValue::Integer(1.try_into().unwrap())]
    );
    assert_eq!(found[0][3], text("constraint_violated"));
    assert!(
        matches!(&found[0][4], SqlValue::Text(detail) if detail.contains("unique")),
        "the record names the constraint: {:?}",
        found[0][4]
    );
    assert_eq!(drain_row(f, &f.workspace).await, before);
    assert_eq!(wake_messages(&runtime, &case.wake).await, 1);
    assert_fixture_attempt_ids_are_uuid_v7(
        f,
        "a_unique_constraint_on_the_drain_batch_is_recorded_as_constraint_violated",
        &[],
    )
    .await;
    f.host.close().await;
}

#[tokio::test]
async fn remote_down_never_masks_central_down_across_two_workspaces() {
    let case = case().await;
    let f = &case.fixture;
    let other = second_workspace(f, 8610).await;
    drain_due_at(f, &other, case.at - chrono::Duration::seconds(1)).await;
    f.workspace_execute(
        "ALTER TABLE changes RENAME TO outage_changes",
        vec![],
        central_storage::RowExpectation::Any,
    )
    .await;
    fault(f, &other, EngineCommitFaultKind::Unavailable).await;

    run(&case).await;

    let runtime = runtime(f).await;
    assert!(
        !runtime.engine_commit_fault_pending().await,
        "W2 met central down"
    );
    assert!(
        record(&runtime, DRAINS, "all").await.is_empty(),
        "CentralDown wins: no record"
    );
    assert_fixture_attempt_ids_are_uuid_v7(
        f,
        "remote_down_never_masks_central_down_across_two_workspaces",
        &[],
    )
    .await;
    f.host.close().await;
}

#[tokio::test]
async fn a_committed_workspace_stays_committed_when_another_violates_a_constraint() {
    let case = case().await;
    let f = &case.fixture;
    let other = second_workspace(f, 8620).await;
    drain_due_at(f, &other, case.at - chrono::Duration::seconds(1)).await;
    collide_drain_updates(f, 8631, &[&other], case.at).await;
    let other_before = drain_row(f, &other).await;

    run(&case).await;

    let runtime = runtime(f).await;
    assert_eq!(
        drain_row(f, &f.workspace).await[1],
        SqlValue::Null,
        "W1 stays committed"
    );
    assert_eq!(drain_row(f, &other).await, other_before, "W2 rolled back");
    let found = record(&runtime, DRAINS, "all").await;
    assert_eq!(found[0][3], text("constraint_violated"));
    assert_fixture_attempt_ids_are_uuid_v7(
        f,
        "a_committed_workspace_stays_committed_when_another_violates_a_constraint",
        &[],
    )
    .await;
    f.host.close().await;
}
