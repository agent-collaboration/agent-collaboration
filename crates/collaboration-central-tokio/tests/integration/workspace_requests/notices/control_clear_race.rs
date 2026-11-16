//! R3/R4 and P5 change 5 on the native host (code review C3): a due drain
//! unit's success clears only the control record it saw at its gate, so a
//! request-drain failure written after that gate survives.
use super::super::engine_evidence::{
    assert_fixture_attempt_ids_are_uuid_v7, witness_fixture_attempt_ids,
};
use super::super::engine_v2_support::{next_mark, raw, rows, runtime, stamp, whole_record};
use super::*;
use collaboration_central_core::{
    EngineCommitFault, EngineCommitFaultKind, engine_probe::InstanceKey,
};

const DRAINS: &str = "legacy_workspace_drains";

#[tokio::test]
async fn an_older_drain_success_never_clears_a_newer_request_drain_failure() {
    let f = Fixture::new().await;
    witness_fixture_attempt_ids(&f).await;
    f.area(9900).await;
    let created = call(
        &f.host,
        &f.caller,
        None,
        9901,
        m::WorkspaceOperation::CreateWorkspace {
            name: name("Late failing workspace"),
        },
    )
    .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace }) =
        created.result
    else {
        panic!("second workspace: {created:?}")
    };
    let late = workspace.workspace_id;
    let runtime = runtime(&f).await;
    raw(
        &runtime,
        "UPDATE machines SET allowed_workspaces=? WHERE machine_id=?",
        vec![
            text(serde_json::to_string(&[&f.workspace, &late]).unwrap()),
            text(f.caller.caller.machine_id()),
        ],
    )
    .await;
    let at = next_mark(f.clock.utc_now());
    raw(
        &runtime,
        "INSERT INTO workspace_drains(workspace_id,drained_through_position,drain_due_at,created_at,updated_at) VALUES (?,0,?,?,?) ON CONFLICT(workspace_id) DO UPDATE SET drain_due_at=excluded.drain_due_at",
        vec![
            text(&f.workspace),
            stamp(at - chrono::Duration::seconds(1)),
            stamp(at),
            stamp(at),
        ],
    )
    .await;
    assert!(
        whole_record(&runtime, DRAINS, "all").await.is_empty(),
        "the due drain's gate sees no record"
    );

    // Due drain A drains the fixture workspace and is held after its success,
    // before it clears the record it observed (none).
    let mut held = runtime
        .pause_points()
        .hold_engine(PausePoint::BeforeControlClear, DRAINS);
    f.clock.set_utc(at);
    let batch = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    held.until_held().await;

    // A write in the late workspace commits; its own immediate drain fails and
    // records the shared unit's failure.
    runtime
        .statement_host()
        .inject_engine_commit_fault(EngineCommitFault {
            key: InstanceKey(late.to_string()),
            kind: EngineCommitFaultKind::DatabaseCode(19),
        });
    let write = call(
        &f.host,
        &f.caller,
        Some(late.clone()),
        9902,
        m::WorkspaceOperation::CreateArea {
            name: name("Late write"),
        },
    )
    .await;
    assert!(
        matches!(write.result, m::WorkspaceResult::Done(_)),
        "{write:?}"
    );
    let newer = whole_record(&runtime, DRAINS, "all").await;
    assert_eq!(newer.len(), 1, "the request drain failure is recorded");
    assert_eq!(newer[0][3], text("failing"));

    held.release();
    batch.await.unwrap().unwrap();
    assert_eq!(
        whole_record(&runtime, DRAINS, "all").await,
        newer,
        "the newer record, its next try and its history survive the older success"
    );
    assert!(
        matches!(
            rows(
                &runtime,
                "SELECT drain_due_at FROM workspace_drains WHERE workspace_id=?",
                vec![text(&late)],
            )
            .await[0][0],
            SqlValue::Text(_)
        ),
        "the late workspace is still undrained"
    );
    assert_fixture_attempt_ids_are_uuid_v7(
        &f,
        "an_older_drain_success_never_clears_a_newer_request_drain_failure",
        &[],
    )
    .await;
    f.host.close().await;
}
