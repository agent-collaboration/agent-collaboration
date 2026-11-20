//! V12 (P5 change 6) on the workerd host: the due batch's drain unit skips a
//! workspace whose turn a held write holds, so another request never waits on it.
use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
use workspace_model::central_mail::{CentralMailOperation, CentralMailRequest, CentralMailResult};

async fn drain_due(
    runtime: &super::super::workerd_host::RemoteRuntime,
    workspace: &m::WorkspaceId,
) -> SqlValue {
    runtime
        .read(Statement {
            sql: "SELECT drain_due_at FROM mail_workspace_drains WHERE workspace_id=?",
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
    let runtime = f.host.central(&f.caller.organization_id).await;
    let start = f.clock.0.load(Ordering::SeqCst);
    // The other workspace has a past drain due that only the due batch will drain.
    let past = chrono::DateTime::from_timestamp_millis(start - 1_000)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    assert!(matches!(
        runtime
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "INSERT INTO mail_workspace_drains(workspace_id,drained_through_position,drain_due_at,created_at,updated_at) VALUES (?,0,?,?,?) ON CONFLICT(workspace_id) DO UPDATE SET drain_due_at=excluded.drain_due_at",
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
    let pause = serde_json::json!({"workspace":f.workspace,"point":"afterStoreApplied"});
    let _: bool = runtime.control("hold", &pause).await.unwrap();
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
                            description: "Test description".to_owned().try_into().unwrap(),
                        },
                    },
                )
                .await
                .unwrap()
        }
    });
    let _: bool = runtime.control("until-held", &pause).await.unwrap();
    let w1_due = drain_due(&runtime, &f.workspace).await;
    assert!(
        matches!(w1_due, SqlValue::Text(_)),
        "A set its +5 s backstop"
    );
    f.clock.0.store(start + 31_000, Ordering::SeqCst);

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
                    reply_to: None,
                    sent_at: "2100-01-01T00:00:00Z".parse().unwrap(),
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
    let records = runtime.read(Statement {
        sql: "SELECT COUNT(*) FROM engine_work_attempts WHERE machine_kind='legacy_workspace_drains'",
        args: vec![],
    }).await.unwrap();
    assert_eq!(
        records.rows,
        vec![vec![SqlValue::Integer(0.try_into().unwrap())]],
        "a skipped workspace is not a failure"
    );
    let _: bool = runtime.control("release", &pause).await.unwrap();
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
