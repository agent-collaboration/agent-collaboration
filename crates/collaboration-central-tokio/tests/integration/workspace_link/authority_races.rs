//! Force revocation between reads and the applying turn on the actual public socket.
use super::fixture::{Fixture, INSTALLATION, receive, send};
use central_link_protocol::{DataFrame, LinkFrame, LinkRequest, ReplyPayload, RequestPayload};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use collaboration_central_core::{MachineLoginControl, machine_login_control};
use fleet_runtime::PausePoint;
use serde_json::json;
use workspace_model::{Caller, RefusalReason, WorkspaceAnswer, WorkspaceReply, WorkspaceResult};

fn frame(correlation: &str, caller: &Caller, operation: serde_json::Value) -> Result<LinkFrame> {
    Ok(LinkFrame::Data(DataFrame::Request {
        request: Box::new(LinkRequest::new(
            correlation.parse()?,
            caller.clone(),
            RequestPayload::Workspace(serde_json::from_value(operation)?),
        )?),
    }))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The real socket, owned-store and forced-order assertions must fail visibly."
)]
#[tokio::test]
async fn revocation_after_rule_reads_refuses_before_workspace_or_drain_effects() -> Result<()> {
    // Arrange: authenticate normally, then create one permitted empty workspace.
    let fixture = Fixture::new().await?;
    let mut socket = fixture.connect().await?;
    assert!(matches!(
        receive(&mut socket).await?,
        LinkFrame::Data(DataFrame::Welcome { .. })
    ));
    let caller = super::link_support::caller()?;
    send(
        &mut socket,
        &frame(
            "019f0000-0000-7000-8000-00000000cf01",
            &caller,
            json!({
                "requestId":"019f0000-0000-7000-8000-00000000cf02", "workspaceId":null,
                "operation":{"kind":"createWorkspace","name":"Revocation boundary"}
            }),
        )?,
    )
    .await?;
    let LinkFrame::Data(DataFrame::Reply { result, .. }) = receive(&mut socket).await? else {
        return Err("missing workspace Reply".into());
    };
    let ReplyPayload::Workspace(WorkspaceReply {
        result: WorkspaceResult::Done(WorkspaceAnswer::WorkspaceCreated { workspace }),
        ..
    }) = *result
    else {
        return Err("workspace creation failed".into());
    };
    let organization = signed::ORGANIZATION.to_owned().try_into()?;
    let runtime = fixture.host.central(&organization).await?;
    let allowed = serde_json::to_string(&vec![workspace.workspace_id.clone()])?;
    assert!(matches!(
        runtime
            .runtime()
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE login_machines SET allowed_workspaces=? WHERE machine_id=?",
                        args: vec![SqlValue::Text(allowed), SqlValue::Text(INSTALLATION.into())]
                    },
                    expect: RowExpectation::Exactly(1)
                }],
                alarm: AlarmUpdate::Unchanged,
            })
            .await?,
        BatchOutcome::Applied { .. }
    ));
    let connection = runtime
        .admit_machine_connection(&fixture.token)
        .await?
        .map_err(|_| "fixture admission refused")?;
    let mut held = runtime
        .runtime()
        .pause_points()
        .hold(&workspace.workspace_id, PausePoint::AfterReads);

    // Act: the real request is held after its reads; the owning revocation commits first.
    send(&mut socket, &frame("019f0000-0000-7000-8000-00000000cf03", &caller, json!({
        "requestId":"019f0000-0000-7000-8000-00000000cf04", "workspaceId":workspace.workspace_id,
        "operation":{"kind":"createArea","name":"Must not commit","description":"Test description"}
    }))?).await?;
    tokio::select! {
        () = held.until_held() => {},
        early = receive(&mut socket) => return Err(format!("request ended before the forced read boundary: {early:?}").into()),
    }
    let revoked = machine_login_control(
        runtime.as_ref(),
        MachineLoginControl::Revoke {
            person_id: signed::OWNER.to_owned().try_into()?,
            operation_id: "019f0000-0000-7000-8000-00000000cf05"
                .to_owned()
                .try_into()?,
            machine_id: INSTALLATION.to_owned().try_into()?,
            enrollment_id: connection.enrollment_id().clone(),
        },
    )
    .await?;
    assert!(revoked.is_some());
    held.release();
    let response = receive(&mut socket).await?;
    assert!(
        matches!(response, LinkFrame::Data(DataFrame::Reply { result, .. }) if matches!(*result,
        ReplyPayload::Workspace(WorkspaceReply { result:WorkspaceResult::Refused(RefusalReason::MachineRevoked), .. })))
    );

    // Assert: admission lost the race, so no workspace or drain deadline may be created.
    let drains = runtime
        .runtime()
        .statement_host()
        .read(Statement {
            sql: "SELECT workspace_id FROM mail_workspace_drains WHERE workspace_id=?",
            args: vec![SqlValue::Text(workspace.workspace_id.to_string())],
        })
        .await?;
    assert!(
        drains.rows.is_empty(),
        "refused in-flight request must not preset a drain"
    );
    let store = collaboration_central_tokio::ObjectStorage::new(
        fixture
            .root_path()
            .join("modern/workspace-stores")
            .join(format!("{}.sqlite", workspace.workspace_id)),
        std::sync::Arc::new(fleet_storage_schema::workspace_migrations::workspace_migrator()),
    );
    let areas = store
        .read(Statement {
            sql: "SELECT area_id FROM workspace_areas",
            args: vec![],
        })
        .await?;
    assert!(areas.rows.is_empty());
    let log = store
        .read(Statement {
            sql: "SELECT request_id FROM workspace_request_log",
            args: vec![],
        })
        .await?;
    assert!(log.rows.is_empty());
    store.close().await;
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The queued request, correlated refusal and owning-store assertions must fail visibly."
)]
#[tokio::test]
async fn revocation_committing_while_request_waits_for_applying_turn_prevents_writes() -> Result<()>
{
    // Arrange: authenticate the link and allow one empty workspace.
    let fixture = Fixture::new().await?;
    let mut socket = fixture.connect().await?;
    assert!(matches!(
        receive(&mut socket).await?,
        LinkFrame::Data(DataFrame::Welcome { .. })
    ));
    let caller = super::link_support::caller()?;
    send(
        &mut socket,
        &frame(
            "019f0000-0000-7000-8000-00000000cf11",
            &caller,
            json!({
                "requestId":"019f0000-0000-7000-8000-00000000cf12", "workspaceId":null,
                "operation":{"kind":"createWorkspace","name":"Applying turn"}
            }),
        )?,
    )
    .await?;
    let LinkFrame::Data(DataFrame::Reply { result, .. }) = receive(&mut socket).await? else {
        return Err("missing workspace creation reply".into());
    };
    let ReplyPayload::Workspace(WorkspaceReply {
        result: WorkspaceResult::Done(WorkspaceAnswer::WorkspaceCreated { workspace }),
        ..
    }) = *result
    else {
        return Err("workspace creation failed".into());
    };
    let workspace = workspace.workspace_id;
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let execution = runtime.runtime();
    let central = execution.statement_host();
    assert!(matches!(
        central
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE login_machines SET allowed_workspaces=? WHERE machine_id=?",
                        args: vec![
                            SqlValue::Text(serde_json::to_string(&[&workspace])?),
                            SqlValue::Text(INSTALLATION.into())
                        ],
                    },
                    expect: RowExpectation::Exactly(1),
                }],
                alarm: AlarmUpdate::Unchanged,
            })
            .await?,
        BatchOutcome::Applied { .. }
    ));
    let mut after_reads = execution
        .pause_points()
        .hold(&workspace, PausePoint::AfterReads);

    // Act: park the request after current authority and workspace reads have succeeded.
    let correlation = "019f0000-0000-7000-8000-00000000cf13";
    send(&mut socket, &frame(correlation, &caller, json!({
        "requestId":"019f0000-0000-7000-8000-00000000cf14","workspaceId":workspace,
        "operation":{"kind":"createArea","name":"Refused while queued","description":"Test description"}
    }))?).await?;
    tokio::select! {
        () = after_reads.until_held() => {},
        early = receive(&mut socket) => return Err(format!("request ended before the read pause: {early:?}").into()),
    }
    let blocking_turn = execution.take_central_turn().await;
    // The central mutex grants turns in queue order. Poll revocation into the queue first.
    let mut revoke = std::pin::pin!(machine_login_control(
        runtime.as_ref(),
        MachineLoginControl::Revoke {
            person_id: signed::OWNER.to_owned().try_into()?,
            operation_id: "019f0000-0000-7000-8000-00000000cf15"
                .to_owned()
                .try_into()?,
            machine_id: INSTALLATION.to_owned().try_into()?,
            enrollment_id: fixture.enrollment_id.clone(),
        },
    ));
    tokio::select! {
        () = execution.until_central_waiting(1) => {},
        outcome = &mut revoke => return Err(format!("revocation bypassed the held turn: {outcome:?}").into()),
    }
    after_reads.release();
    // Observing both waiters while the lock is still held forces the request to wait
    // while live, and ensures that revocation will own the turn immediately before it.
    tokio::select! {
        () = execution.until_central_waiting(2) => {},
        early = receive(&mut socket) => return Err(format!("request bypassed the applying turn: {early:?}").into()),
    }
    drop(blocking_turn);
    assert!(revoke.await?.is_some());

    // Assert: the same request is refused and leaves no business, ledger or drain write.
    let reply = receive(&mut socket).await?;
    let LinkFrame::Data(DataFrame::Reply {
        correlation_id,
        result,
    }) = reply
    else {
        return Err(format!("expected a correlated workspace refusal, received {reply:?}").into());
    };
    assert_eq!(correlation_id, correlation.parse()?);
    let ReplyPayload::Workspace(reply) = *result else {
        return Err("expected a workspace result".into());
    };
    assert_eq!(
        reply.result,
        WorkspaceResult::Refused(RefusalReason::MachineRevoked)
    );
    let drains = central
        .read(Statement {
            sql: "SELECT workspace_id FROM mail_workspace_drains WHERE workspace_id=?",
            args: vec![SqlValue::Text(workspace.to_string())],
        })
        .await?;
    assert!(drains.rows.is_empty(), "refused request preset a drain");
    let store = collaboration_central_tokio::ObjectStorage::new(
        fixture
            .root_path()
            .join("modern/workspace-stores")
            .join(format!("{workspace}.sqlite")),
        std::sync::Arc::new(fleet_storage_schema::workspace_migrations::workspace_migrator()),
    );
    for sql in [
        "SELECT area_id FROM workspace_areas",
        "SELECT request_id FROM workspace_request_log",
    ] {
        let rows = store.read(Statement { sql, args: vec![] }).await?;
        assert!(rows.rows.is_empty(), "refused request wrote rows for {sql}");
    }
    store.close().await;
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    fixture.close().await
}
