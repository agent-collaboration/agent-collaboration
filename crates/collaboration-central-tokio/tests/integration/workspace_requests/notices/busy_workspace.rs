//! V12 (P5 change 6) on the native host: the due batch's drain unit skips a
//! workspace whose turn a held write holds, so another request never waits on it.
use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
use workspace_model::central_mail::{CentralMailOperation, CentralMailRequest, CentralMailResult};

async fn drain_due(
    runtime: &std::sync::Arc<
        collaboration_central_core::WorkspaceRuntime<
            collaboration_central_tokio::ObjectStorage,
            collaboration_central_tokio::TokioWorkspaceStores,
        >,
    >,
    workspace: &m::WorkspaceId,
) -> SqlValue {
    runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT drain_due_at FROM workspace_drains WHERE workspace_id=?",
            args: vec![text(workspace)],
        })
        .await
        .unwrap()
        .rows[0][0]
        .clone()
}

#[tokio::test]
async fn a_held_write_on_one_workspace_never_delays_another_requests_reply() {
    let f = Fixture::new().await;
    let created = call(
        &f.host,
        &f.caller,
        None,
        9600,
        m::WorkspaceOperation::CreateWorkspace {
            name: name("Busy second workspace"),
        },
    )
    .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace }) =
        created.result
    else {
        panic!("second workspace")
    };
    let other = workspace.workspace_id;
    let runtime = f.host.central(&f.caller.organization_id).await.unwrap();
    // The other workspace has a past drain due that only the due batch will drain.
    let past = (f.clock.utc_now() - chrono::Duration::seconds(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "INSERT INTO workspace_drains(workspace_id,drained_through_position,drain_due_at,created_at,updated_at) VALUES (?,0,?,?,?) ON CONFLICT(workspace_id) DO UPDATE SET drain_due_at=excluded.drain_due_at",
                        args: vec![text(&other), text(&past), text(&past), text(&past)],
                    },
                    expect: RowExpectation::Exactly(1),
                }],
                alarm: AlarmUpdate::Unchanged,
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));

    // Write A commits in W1 and is held holding W1's turn, before its own drain.
    let mut held = runtime
        .pause_points()
        .hold(&f.workspace, PausePoint::AfterStoreApplied);
    let pending = tokio::spawn({
        let runtime = runtime.clone();
        let caller = f.caller.clone();
        let workspace = f.workspace.clone();
        async move {
            runtime
                .request(
                    &caller,
                    m::WorkspaceRequest {
                        request_id: Some(id(9601)),
                        workspace_id: Some(workspace),
                        operation: m::WorkspaceOperation::CreateArea {
                            name: name("Held write"),
                        },
                    },
                )
                .await
                .unwrap()
        }
    });
    held.until_held().await;
    let w1_due = drain_due(&runtime, &f.workspace).await;
    assert!(
        matches!(w1_due, SqlValue::Text(_)),
        "A set its +5 s backstop"
    );
    let start = f.clock.utc_now();
    f.clock.set_utc(start + chrono::Duration::seconds(31));

    // Unrelated request B claims the due batch and completes while A is held.
    let reply = f
        .host
        .request_central(
            &f.caller,
            CentralMailRequest {
                request_id: id(9602),
                operation: CentralMailOperation::SendMessage {
                    to: session(&f.caller),
                    body: "unrelated".to_owned().try_into().unwrap(),
                    delivery: workspace_model::central_mail::MessageDelivery::Auto,
                },
            },
        )
        .await
        .unwrap();

    assert!(matches!(reply, CentralMailResult::Done(_)), "{reply:?}");
    assert_eq!(
        drain_due(&runtime, &f.workspace).await,
        w1_due,
        "the busy workspace was skipped: its drain_due_at stays"
    );
    assert_eq!(
        drain_due(&runtime, &other).await,
        SqlValue::Null,
        "other due workspaces drain"
    );
    let records = runtime.statement_host().read(Statement {
        sql: "SELECT COUNT(*) FROM central_work_attempts WHERE machine_kind='legacy_workspace_drains'",
        args: vec![],
    }).await.unwrap();
    assert_eq!(
        records.rows,
        vec![vec![SqlValue::Integer(0.try_into().unwrap())]],
        "a skipped workspace is not a failure"
    );
    held.release();
    assert!(matches!(
        pending.await.unwrap().result,
        m::WorkspaceResult::Done(_)
    ));
    assert_eq!(
        drain_due(&runtime, &f.workspace).await,
        SqlValue::Null,
        "A's own drain drains W1 once released"
    );
    f.host.close().await;
}
