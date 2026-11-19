//! Force revocation between reads and the applying turn on the actual public socket.
use super::fixture::{Fixture, INSTALLATION, receive, send};
use central_link_protocol::{DataFrame, LinkFrame, LinkRequest, ReplyPayload, RequestPayload};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use collaboration_central_core::{MachineLoginControl, PausePoint, machine_login_control};
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
    let caller = Caller::Person {
        machine_id: INSTALLATION.parse()?,
        person_id: signed::OWNER.parse()?,
    };
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
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE machines SET allowed_workspaces=? WHERE machine_id=?",
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
        .statement_host()
        .read(Statement {
            sql: "SELECT workspace_id FROM workspace_drains WHERE workspace_id=?",
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
        std::sync::Arc::new(collaboration_central_core::workspace_migrations::workspace_migrator()),
    );
    let areas = store
        .read(Statement {
            sql: "SELECT area_id FROM areas",
            args: vec![],
        })
        .await?;
    assert!(areas.rows.is_empty());
    let log = store
        .read(Statement {
            sql: "SELECT request_id FROM request_log",
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
