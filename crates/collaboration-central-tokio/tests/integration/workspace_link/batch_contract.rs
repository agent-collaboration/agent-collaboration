//! One native batch preserves ordered independent replies across a semantic refusal.
use super::fixture::{Fixture, INSTALLATION, receive, send};
use central_link_client::{ClientAction, ClientEvent, LinkClient};
use central_link_protocol::{
    DataFrame, LinkFrame, LinkRequest, LinkRequestBatch, ReplyPayload, RequestPayload,
};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use collaboration_central_tokio::ObjectStorage;
use serde_json::json;
use std::{collections::BTreeSet, sync::Arc};
use workspace_model::{
    Caller, ItemRef, SessionAddress, WorkspaceAnswer, WorkspaceReply, WorkspaceResult,
    central_mail::{CentralMailRefusal, CentralMailResult, TimingRequirement},
};

fn request_from_client(frame: LinkFrame) -> Result<LinkRequest> {
    let LinkFrame::Data(DataFrame::Request { request }) = frame else {
        return Err("client did not register a Request".into());
    };
    Ok(*request)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Real batch replies and owning SQLite rows must match the independent member contracts."
)]
#[tokio::test]
async fn native_batch_keeps_order_and_continues_after_a_middle_semantic_refusal() -> Result<()> {
    // Arrange: real signed admission and an ordinary installation-addressed session.
    let fixture = Fixture::new().await?;
    let target: SessionAddress = serde_json::from_value(json!({
        "machineId":INSTALLATION,"endpoint":"codex-local",
        "sessionId":"019f0000-0000-7000-8000-00000000cf07"
    }))?;
    let caller = Caller::Session(target.clone());
    let mut client = LinkClient::new(INSTALLATION.parse()?, fixture.clock.clone());
    let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
        return Err("client did not start a connection attempt".into());
    };
    let mut socket = fixture.connect().await?;
    if client.receive(&attempt, receive(&mut socket).await?)? != ClientEvent::Welcomed {
        return Err("normal installation did not receive Welcome".into());
    }
    let create = client.begin_request(
        caller.clone(),
        RequestPayload::Workspace(serde_json::from_value(json!({
            "requestId":"019f0000-0000-7000-8000-00000000d401","workspaceId":null,
            "operation":{"kind":"createWorkspace","name":"Batch owning workspace"}
        }))?),
    )?;
    send(&mut socket, &create).await?;
    let ClientEvent::Reply {
        result:
            ReplyPayload::Workspace(WorkspaceReply {
                result: WorkspaceResult::Done(WorkspaceAnswer::WorkspaceCreated { workspace }),
                ..
            }),
        ..
    } = client.receive(&attempt, receive(&mut socket).await?)?
    else {
        return Err("permitted workspace fixture creation failed".into());
    };
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let allowance = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "UPDATE machines SET allowed_workspaces=? WHERE installation_id=?",
                    args: vec![
                        SqlValue::Text(serde_json::to_string(&vec![
                            workspace.workspace_id.clone(),
                        ])?),
                        SqlValue::Text(INSTALLATION.into()),
                    ],
                },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await?;
    if !matches!(allowance, BatchOutcome::Applied { .. }) {
        return Err("batch workspace allowance fixture did not apply".into());
    }
    let first = request_from_client(client.begin_request(
        caller.clone(),
        RequestPayload::Workspace(serde_json::from_value(json!({
            "requestId":"019f0000-0000-7000-8000-00000000d402","workspaceId":workspace.workspace_id,
            "operation":{"kind":"createArea","name":"Batch area A","description":"Test description"}
        }))?),
    )?)?;
    // The timestamp is on the five-minute grid and is earlier than NOW_SECONDS.
    // This must reach the semantic FutureTime refusal, not fail decoding or caller checks.
    let middle = request_from_client(client.begin_request(caller.clone(), RequestPayload::Mail(serde_json::from_value(json!({
        "requestId":"019f0000-0000-7000-8000-00000000d403",
        "operation":{"kind":"setWake","target":target,"dueAt":"2030-03-17T17:45:00Z","text":"Past batch wake"}
    }))?))?)?;
    let last = request_from_client(client.begin_request(
        caller,
        RequestPayload::Workspace(serde_json::from_value(json!({
            "requestId":"019f0000-0000-7000-8000-00000000d404","workspaceId":workspace.workspace_id,
            "operation":{"kind":"createArea","name":"Batch area B","description":"Test description"}
        }))?),
    )?)?;
    let requests = vec![first, middle, last];
    let correlations: Vec<_> = requests
        .iter()
        .map(|request| request.correlation_id().clone())
        .collect();
    assert_eq!(correlations.iter().collect::<BTreeSet<_>>().len(), 3);
    let durable_ids: BTreeSet<_> = requests
        .iter()
        .map(|request| request.payload().write_request_id().cloned())
        .collect();
    assert_eq!(durable_ids.len(), 3);
    assert!(!durable_ids.contains(&None));

    // Act: all three pending client calls cross the public socket in one frame.
    send(
        &mut socket,
        &LinkFrame::Data(DataFrame::Batch {
            requests: LinkRequestBatch::new(requests)?,
        }),
    )
    .await?;
    let mut replies = Vec::new();
    for expected in correlations {
        let ClientEvent::Reply {
            correlation_id,
            result,
        } = client.receive(&attempt, receive(&mut socket).await?)?
        else {
            return Err("batch member did not produce an individual correlated Reply".into());
        };
        assert_eq!(
            correlation_id, expected,
            "batch replies must follow frame order"
        );
        replies.push(result);
    }

    // Assert: the middle semantic refusal does not roll back A or prevent B.
    let [
        ReplyPayload::Workspace(first),
        ReplyPayload::Mail(middle),
        ReplyPayload::Workspace(last),
    ] = replies.as_slice()
    else {
        return Err("batch reply payload families did not match their members".into());
    };
    let WorkspaceResult::Done(WorkspaceAnswer::Created {
        item: ItemRef::Area(first_area),
        position: first_position,
    }) = &first.result
    else {
        return Err(format!("first area write did not succeed: {:?}", first.result).into());
    };
    assert_eq!(first_position.get(), 1);
    assert_eq!(
        middle,
        &CentralMailResult::Refused(CentralMailRefusal::TimingViolation {
            field: "dueAt".into(),
            requirement: TimingRequirement::FutureTime,
        })
    );
    let WorkspaceResult::Done(WorkspaceAnswer::Created {
        item: ItemRef::Area(last_area),
        position: last_position,
    }) = &last.result
    else {
        return Err(format!(
            "last area write did not survive middle refusal: {:?}",
            last.result
        )
        .into());
    };
    assert_eq!(last_position.get(), 2);
    assert_ne!(first_area, last_area);
    let store = ObjectStorage::new(
        fixture
            .root_path()
            .join("modern/workspace-stores")
            .join(format!("{}.sqlite", workspace.workspace_id)),
        Arc::new(collaboration_central_core::workspace_migrations::workspace_migrator()),
    );
    let areas = store
        .read(Statement {
            sql: "SELECT area_id,name FROM areas ORDER BY name",
            args: vec![],
        })
        .await?;
    assert_eq!(
        areas.rows,
        vec![
            vec![
                SqlValue::Text(first_area.to_string()),
                SqlValue::Text("Batch area A".into())
            ],
            vec![
                SqlValue::Text(last_area.to_string()),
                SqlValue::Text("Batch area B".into())
            ],
        ]
    );
    let logged = store
        .read(Statement {
            sql: "SELECT request_id FROM request_log ORDER BY request_id",
            args: vec![],
        })
        .await?;
    assert_eq!(
        logged.rows,
        vec![
            vec![SqlValue::Text(
                "019f0000-0000-7000-8000-00000000d402".into()
            )],
            vec![SqlValue::Text(
                "019f0000-0000-7000-8000-00000000d404".into()
            )],
        ]
    );
    assert!(
        runtime
            .statement_host()
            .read(Statement {
                sql: "SELECT wake_id FROM wakes",
                args: vec![]
            })
            .await?
            .rows
            .is_empty()
    );
    store.close().await;
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    fixture.close().await
}
