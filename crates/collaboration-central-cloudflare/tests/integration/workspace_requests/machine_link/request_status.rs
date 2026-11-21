use super::fixture::{ConnectedLink, Fixture, receive, send, session_caller};
use central_link_protocol::{
    CorrelationId, DataFrame, ReplyPayload, RequestScope, RequestStatusAnswer, RequestStatusQuery,
};
use central_storage::{
    AlarmUpdate, BatchOutcome, CorruptReason, GuardedStatement, ReadOutcome, RowExpectation,
    SqlValue, Statement, StatementBatch, StoreFailure,
};
use collaboration_central_conformance::ConformanceResult as Result;
use fleet_contracts::WorkspaceResponse;
use fleet_rpc::Envelope;
use futures_util::StreamExt;
use reqwest_websocket::{CloseCode, Message};
use serde::Deserialize;
use serde_json::{Value, json};
use workspace_model::{
    RequestId, WorkspaceAnswer, WorkspaceId, WorkspaceOperation, WorkspaceReply, WorkspaceRequest,
    WorkspaceResult,
};

#[tokio::test]
async fn worker_machine_link_reads_the_retained_decision_from_the_selected_store() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = query_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn query_journey(fixture: &Fixture) -> Result<()> {
    let mut link = ConnectedLink::open(fixture).await?;
    let organization_request: RequestId = "019f0000-0000-7000-8000-00000000cf11".parse()?;
    let organization_reply = call(
        &mut link,
        &organization_request,
        None,
        WorkspaceOperation::CreateWorkspace {
            name: "Status workspace".to_owned().try_into()?,
        },
    )
    .await?;
    let ReplyPayload::Workspace(WorkspaceReply {
        result: WorkspaceResult::Done(WorkspaceAnswer::WorkspaceCreated { workspace }),
        ..
    }) = &organization_reply
    else {
        return Err("create did not retain a workspace".into());
    };
    let workspace = &workspace.workspace_id;
    let runtime = fixture.runtime();

    let before_refusal = runtime
        .control::<[u64; 2]>("raw-workspace-call-counts", &())
        .await?;
    check_status(
        &mut link,
        &organization_request,
        RequestScope::Workspace(workspace.clone()),
        0x12,
        RequestStatusAnswer::NotFound,
    )
    .await?;
    assert_eq!(
        runtime
            .control::<[u64; 2]>("raw-workspace-call-counts", &())
            .await?,
        before_refusal
    );

    fixture.allow_workspace(workspace).await?;
    let workspace_request: RequestId = "019f0000-0000-7000-8000-00000000cf13".parse()?;
    let workspace_reply = call(
        &mut link,
        &workspace_request,
        Some(workspace.clone()),
        WorkspaceOperation::CreateArea {
            name: "One area".to_owned().try_into()?,
            description: "Retained answer".to_owned().try_into()?,
        },
    )
    .await?;
    assert!(matches!(
        &workspace_reply,
        ReplyPayload::Workspace(WorkspaceReply {
            result: WorkspaceResult::Done(WorkspaceAnswer::Created { .. }),
            ..
        })
    ));
    let before_rows = stored_state(fixture, workspace).await?;
    let [legacy, typed] = runtime
        .control::<[u64; 2]>("raw-workspace-call-counts", &())
        .await?;
    for (scope, id, correlation, answer, expected_typed) in [
        (
            RequestScope::Organization,
            &organization_request,
            0x14,
            RequestStatusAnswer::Answered(organization_reply.clone()),
            typed,
        ),
        (
            RequestScope::Workspace(workspace.clone()),
            &organization_request,
            0x15,
            RequestStatusAnswer::NotFound,
            typed + 1,
        ),
        (
            RequestScope::Workspace(workspace.clone()),
            &workspace_request,
            0x16,
            RequestStatusAnswer::Answered(workspace_reply.clone()),
            typed + 2,
        ),
    ] {
        check_status(&mut link, id, scope, correlation, answer).await?;
        assert_eq!(
            runtime
                .control::<[u64; 2]>("raw-workspace-call-counts", &())
                .await?,
            [legacy, expected_typed]
        );
    }
    assert_eq!(stored_state(fixture, workspace).await?, before_rows);

    let armed: bool = runtime.control("lose-read", &()).await?;
    assert!(armed);
    let scope = RequestScope::Workspace(workspace.clone());
    let frame = status_frame(&workspace_request, scope.clone(), 0x17)?;
    assert_link_store_failure(&mut link, &frame).await?;
    let mut link = ConnectedLink::open(fixture).await?;
    check_status(
        &mut link,
        &workspace_request,
        scope,
        0x18,
        RequestStatusAnswer::Answered(workspace_reply),
    )
    .await?;
    assert_eq!(
        runtime
            .control::<[u64; 2]>("raw-workspace-call-counts", &())
            .await?,
        [legacy, typed + 4]
    );
    assert_eq!(stored_state(fixture, workspace).await?, before_rows);
    check_corruption(fixture, &mut link, workspace, &workspace_request).await
}

async fn call(
    link: &mut ConnectedLink,
    id: &RequestId,
    workspace_id: Option<WorkspaceId>,
    operation: WorkspaceOperation,
) -> Result<ReplyPayload> {
    let exchange = link
        .call(WorkspaceRequest {
            request_id: Some(id.clone()),
            workspace_id,
            operation,
        })
        .await?;
    match exchange.reply {
        DataFrame::Reply { result, .. } => Ok(*result),
        reply => Err(format!("write did not return a Reply: {reply:?}").into()),
    }
}

async fn check_status(
    link: &mut ConnectedLink,
    id: &RequestId,
    scope: RequestScope,
    correlation: u8,
    expected: RequestStatusAnswer,
) -> Result<()> {
    let request = status_frame(id, scope, correlation)?;
    let DataFrame::RequestStatus { correlation_id, .. } = &request else {
        return Err("status fixture constructed the wrong frame".into());
    };
    send(&mut link.socket, &request).await?;
    assert_eq!(
        receive(&mut link.socket).await?,
        DataFrame::RequestStatusReply {
            correlation_id: correlation_id.clone(),
            result: Box::new(expected),
        }
    );
    Ok(())
}

fn status_frame(id: &RequestId, scope: RequestScope, correlation: u8) -> Result<DataFrame> {
    let correlation_id: CorrelationId =
        format!("019f0000-0000-7000-8000-00000000cf{correlation:02x}").parse()?;
    Ok(DataFrame::RequestStatus {
        correlation_id,
        caller: session_caller()?,
        query: RequestStatusQuery {
            request_id: id.clone(),
            scope,
        },
    })
}

async fn stored_state(
    fixture: &Fixture,
    workspace: &WorkspaceId,
) -> Result<(ReadOutcome, ReadOutcome, u64)> {
    let runtime = fixture.runtime();
    let central = runtime.read(Statement {
        sql: "SELECT (SELECT COUNT(*) FROM access_actor_identities), (SELECT COUNT(*) FROM runtime_request_log)",
        args: vec![],
    }).await?;
    let owning = runtime.inspect_workspace(workspace, Statement {
        sql: "SELECT (SELECT COUNT(*) FROM workspace_request_log), (SELECT COUNT(*) FROM workspace_areas), (SELECT COUNT(*) FROM workspace_changes)",
        args: vec![],
    }).await?;
    let completions = runtime.control("completion-attempts", &()).await?;
    Ok((central, owning, completions))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressReceipt {
    status: u16,
    body: String,
    storage_before: [bool; 2],
    storage_after: [bool; 2],
}

fn workspace_header(workspace: &str) -> Value {
    json!({"kind":"workspaceStore","value":workspace})
}

fn status_probe(physical: &str, header: Value, target: &str, id: &str, actor: &str) -> Value {
    json!({
        "physical":physical, "header":header,
        "envelope":{
            "target":target, "body":{"kind":"readRequestStatus","call":{
                "requestId":id, "actor":actor, "cutoff":"2020-01-01T00:00:00Z"
            }}
        }
    })
}

async fn check_corruption(
    fixture: &Fixture,
    link: &mut ConnectedLink,
    workspace: &WorkspaceId,
    id: &RequestId,
) -> Result<()> {
    let runtime = fixture.runtime();
    let caller = runtime
        .inspect_workspace(
            workspace,
            Statement {
                sql: "SELECT caller FROM workspace_request_log WHERE request_id=?",
                args: vec![SqlValue::Text(id.to_string())],
            },
        )
        .await?;
    let [row] = caller.rows.as_slice() else {
        return Err("workspace write should leave one retained answer".into());
    };
    let [SqlValue::Text(actor)] = row.as_slice() else {
        return Err("workspace answer should retain the writing actor".into());
    };
    assert_ne!(actor, "019f0000-0000-7000-8000-00000000cf60");
    let altered = runtime.workspace_batch(workspace, StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement {
                sql: "UPDATE workspace_request_log SET caller=?, answer=? WHERE request_id=?",
                args: vec![
                    SqlValue::Text("019f0000-0000-7000-8000-00000000cf60".into()),
                    SqlValue::Blob(vec![255]),
                    SqlValue::Text(id.to_string()),
                ],
            },
            expect: RowExpectation::Exactly(1),
        }],
        alarm: AlarmUpdate::Unchanged,
    }).await?;
    assert!(matches!(altered, BatchOutcome::Applied { .. }));
    let receipt: AddressReceipt = runtime
        .control(
            "raw-workspace-rpc-probe",
            &status_probe(
                workspace.as_str(),
                workspace_header(workspace.as_str()),
                workspace.as_str(),
                id.as_str(),
                actor,
            ),
        )
        .await?;
    assert_eq!(receipt.status, 200, "{}", receipt.body);
    assert_eq!(receipt.storage_before, [true, true]);
    assert_eq!(receipt.storage_after, [true, true]);
    let envelope: Envelope<WorkspaceId, WorkspaceResponse> = serde_json::from_str(&receipt.body)?;
    assert_eq!(envelope.target, *workspace);
    let WorkspaceResponse::ReadRequestStatus(answer) = envelope.body;
    assert_eq!(
        answer,
        Err(StoreFailure::Corrupt {
            reason: CorruptReason::BadValue {
                column: "answer".into()
            },
        })
    );

    let frame = status_frame(id, RequestScope::Workspace(workspace.clone()), 0x19)?;
    assert_link_store_failure(link, &frame).await
}

async fn assert_link_store_failure(link: &mut ConnectedLink, frame: &DataFrame) -> Result<()> {
    send(&mut link.socket, frame).await?;
    let close = link.socket.next().await;
    assert!(
        matches!(
            &close,
            Some(Ok(Message::Close { code: CloseCode::Error, reason }))
                if reason == "central unavailable"
        ),
        "expected the real link to report store failure: {close:?}"
    );
    Ok(())
}

#[tokio::test]
async fn workspace_status_rpc_rejects_each_wrong_address_before_migration() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = check_address_mismatches(&fixture).await;
    fixture.close().await?;
    result
}

async fn check_address_mismatches(fixture: &Fixture) -> Result<()> {
    let first = "019f0000-0000-7000-8000-00000000cf21";
    let second = "019f0000-0000-7000-8000-00000000cf22";
    let third = "019f0000-0000-7000-8000-00000000cf23";
    let other = "019f0000-0000-7000-8000-00000000cf20";
    let runtime = fixture.runtime();
    for (case, physical, header, target) in [
        (
            "header namespace",
            first,
            json!({"kind":"central","value":first}),
            first,
        ),
        ("physical object", second, workspace_header(other), other),
        ("envelope target", third, workspace_header(third), other),
    ] {
        let receipt: AddressReceipt = runtime
            .control(
                "raw-workspace-rpc-probe",
                &status_probe(
                    physical,
                    header,
                    target,
                    "019f0000-0000-7000-8000-00000000cf24",
                    "019f0000-0000-7000-8000-00000000cf25",
                ),
            )
            .await?;
        assert_ne!(
            receipt.status, 200,
            "{case} unexpectedly passed: {}",
            receipt.body
        );
        assert_eq!(
            receipt.storage_before,
            [false, false],
            "{case} fixture should start cold"
        );
        assert_eq!(
            receipt.storage_after,
            [false, false],
            "{case} ran before validation"
        );
    }
    Ok(())
}
