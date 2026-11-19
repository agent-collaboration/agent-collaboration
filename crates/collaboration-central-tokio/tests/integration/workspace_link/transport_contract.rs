//! Real links retain each connection's grant, correlate concurrent calls and replay writes.
use super::fixture::{Fixture, INSTALLATION, send};
use super::link_support::{
    caller, connect_client, correlation, create_workspace, created_workspace, workspace_reply,
};
use central_link_protocol::RequestPayload;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use collaboration_central_tokio::ObjectStorage;
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use workspace_model::{ItemRef, WorkspaceAnswer, WorkspaceId, WorkspaceResult};

fn workspace_store(fixture: &Fixture, workspace: &WorkspaceId) -> ObjectStorage {
    ObjectStorage::new(
        fixture
            .root_path()
            .join("modern/workspace-stores")
            .join(format!("{workspace}.sqlite")),
        Arc::new(collaboration_central_core::workspace_migrations::workspace_migrator()),
    )
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Two outstanding requests must retain their own network correlation and payload."
)]
#[tokio::test]
async fn two_in_flight_requests_keep_distinct_correlations_and_payloads() -> Result<()> {
    let fixture = Fixture::new().await?;
    let (mut client, attempt, mut socket) = connect_client(&fixture, &fixture.token).await?;
    let first = client.begin_request(
        caller()?,
        create_workspace("019f0000-0000-7000-8000-00000000d201", "First outstanding")?,
    )?;
    let second = client.begin_request(
        caller()?,
        create_workspace("019f0000-0000-7000-8000-00000000d202", "Second outstanding")?,
    )?;
    let first_id = correlation(&first)?;
    let second_id = correlation(&second)?;
    assert_ne!(first_id, second_id);
    // Both frames cross the public socket before the client consumes either answer.
    send(&mut socket, &first).await?;
    send(&mut socket, &second).await?;
    let mut replies = BTreeMap::new();
    for _ in 0..2 {
        let (correlation, reply) = workspace_reply(&mut client, &attempt, &mut socket).await?;
        assert!(replies.insert(correlation, reply).is_none());
    }
    assert_eq!(
        created_workspace(replies.get(&first_id).ok_or("first correlation lost")?)?
            .name
            .as_str(),
        "First outstanding"
    );
    assert_eq!(
        created_workspace(replies.get(&second_id).ok_or("second correlation lost")?)?
            .name
            .as_str(),
        "Second outstanding"
    );
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT name FROM directory_workspaces ORDER BY name",
            args: vec![],
        })
        .await?;
    assert_eq!(
        rows.rows,
        vec![
            vec![SqlValue::Text("First outstanding".into())],
            vec![SqlValue::Text("Second outstanding".into())]
        ]
    );
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Lost reply and replay assertions compare the real socket with its retained SQLite answer."
)]
#[tokio::test]
async fn lost_reply_after_commit_replays_one_durable_write_on_a_new_correlation() -> Result<()> {
    // Arrange: create a real workspace and establish its explicit allowance precondition.
    let fixture = Fixture::new().await?;
    let (mut client, attempt, mut socket) = connect_client(&fixture, &fixture.token).await?;
    let create = client.begin_request(
        caller()?,
        create_workspace(
            "019f0000-0000-7000-8000-00000000d301",
            "Replay owning store",
        )?,
    )?;
    send(&mut socket, &create).await?;
    let (_, reply) = workspace_reply(&mut client, &attempt, &mut socket).await?;
    let workspace = created_workspace(&reply)?;
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let allowance = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "UPDATE login_machines SET allowed_workspaces=? WHERE machine_id=?",
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
        return Err("workspace allowance fixture did not apply".into());
    }
    let write_id = "019f0000-0000-7000-8000-00000000d302";
    let payload = RequestPayload::Workspace(serde_json::from_value(json!({
        "requestId":write_id,"workspaceId":workspace.workspace_id,
        "operation":{"kind":"createArea","name":"Exactly one area","description":"Test description"}
    }))?);
    runtime.lose_reply_after_apply(write_id.parse()?);

    // Act: the response is unknown only after the actual workspace batch committed.
    let write = client.begin_request(caller()?, payload.clone())?;
    send(&mut socket, &write).await?;
    let (first_id, first_reply) = workspace_reply(&mut client, &attempt, &mut socket).await?;
    assert_eq!(first_id, correlation(&write)?);
    assert_eq!(first_reply.result, WorkspaceResult::OutcomeUnknown);
    let store = workspace_store(&fixture, &workspace.workspace_id);
    let committed = store
        .read(Statement {
            sql: "SELECT request_id,answer FROM workspace_request_log",
            args: vec![],
        })
        .await?;
    let [row] = committed.rows.as_slice() else {
        return Err("expected one committed request row before replay".into());
    };
    let [SqlValue::Text(stored_id), SqlValue::Text(answer)] = row.as_slice() else {
        return Err("committed request answer shape".into());
    };
    assert_eq!(stored_id, write_id);
    let durable_answer: WorkspaceResult = serde_json::from_str(answer)?;
    let WorkspaceResult::Done(WorkspaceAnswer::Created {
        item: ItemRef::Area(area),
        position,
    }) = &durable_answer
    else {
        return Err("committed answer was not the created area".into());
    };
    assert_eq!(position.get(), 1);
    let expected_area = vec![vec![
        SqlValue::Text(area.to_string()),
        SqlValue::Text("Exactly one area".into()),
    ]];
    assert_eq!(
        store
            .read(Statement {
                sql: "SELECT area_id,name FROM workspace_areas",
                args: vec![]
            })
            .await?
            .rows,
        expected_area
    );

    // Assert: a new transport correlation with the same durable id replays that exact answer.
    let retry = client.begin_request(caller()?, payload)?;
    assert_ne!(correlation(&retry)?, first_id);
    send(&mut socket, &retry).await?;
    let (retry_id, replay) = workspace_reply(&mut client, &attempt, &mut socket).await?;
    assert_eq!(retry_id, correlation(&retry)?);
    assert_eq!(replay.result, durable_answer);
    assert_eq!(
        store
            .read(Statement {
                sql: "SELECT area_id,name FROM workspace_areas",
                args: vec![]
            })
            .await?
            .rows,
        expected_area
    );
    assert_eq!(
        store
            .read(Statement {
                sql: "SELECT request_id,answer FROM workspace_request_log",
                args: vec![]
            })
            .await?,
        committed
    );
    store.close().await;
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    fixture.close().await
}
