//! Real links retain each connection's grant, correlate concurrent calls and replay writes.
use super::fixture::{Fixture, INSTALLATION, receive, send};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use central_link_client::{ClientAction, ClientEvent, ConnectionAttempt, LinkClient};
use central_link_protocol::{
    CorrelationId, DataFrame, LinkFrame, ReplyPayload, RequestPayload, TokenRenewalRefusal,
    TokenRenewalResult,
};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use collaboration_central_core::identity_contract::MachineTokenDecision;
use collaboration_central_tokio::ObjectStorage;
use reqwest_websocket::{RequestBuilderExt, WebSocket};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use workspace_model::{
    Caller, ItemRef, RefusalReason, Workspace, WorkspaceAnswer, WorkspaceId, WorkspaceReply,
    WorkspaceResult,
};

fn caller() -> Result<Caller> {
    Ok(Caller::Person {
        machine_id: INSTALLATION.parse()?,
        person_id: signed::OWNER.parse()?,
    })
}

fn create_workspace(request_id: &str, name: &str) -> Result<RequestPayload> {
    Ok(RequestPayload::Workspace(serde_json::from_value(json!({
        "requestId":request_id,"workspaceId":null,
        "operation":{"kind":"createWorkspace","name":name}
    }))?))
}

fn correlation(frame: &LinkFrame) -> Result<CorrelationId> {
    let LinkFrame::Data(DataFrame::Request { request }) = frame else {
        return Err("client did not produce a Request".into());
    };
    Ok(request.correlation_id().clone())
}

async fn connect_client(
    fixture: &Fixture,
    token: &str,
) -> Result<(LinkClient, ConnectionAttempt, WebSocket)> {
    let mut client = LinkClient::new(INSTALLATION.parse()?, fixture.clock.clone());
    let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
        return Err("client did not start a connection attempt".into());
    };
    let mut socket = fixture
        .client()?
        .get(fixture.url(signed::ORGANIZATION))
        .bearer_auth(token)
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    if client.receive(&attempt, receive(&mut socket).await?)? != ClientEvent::Welcomed {
        return Err("authenticated link did not Welcome its installation".into());
    }
    Ok((client, attempt, socket))
}

async fn workspace_reply(
    client: &mut LinkClient,
    attempt: &ConnectionAttempt,
    socket: &mut WebSocket,
) -> Result<(CorrelationId, WorkspaceReply)> {
    let ClientEvent::Reply {
        correlation_id,
        result: ReplyPayload::Workspace(reply),
    } = client.receive(attempt, receive(socket).await?)?
    else {
        return Err("expected a correlated workspace Reply".into());
    };
    Ok((correlation_id, reply))
}

fn created_workspace(reply: &WorkspaceReply) -> Result<Workspace> {
    let WorkspaceResult::Done(WorkspaceAnswer::WorkspaceCreated { workspace }) = &reply.result
    else {
        return Err(format!("workspace creation did not succeed: {:?}", reply.result).into());
    };
    Ok(workspace.clone())
}

fn renewed_token(original: &str, grant: &MachineTokenDecision, client_id: &str) -> Result<String> {
    let MachineTokenDecision::Grant {
        installation,
        enrollment_id,
        decided_at,
        grant_expires_at,
        ..
    } = grant
    else {
        return Err("real renewal grant refused".into());
    };
    let encoded = original.split('.').nth(1).ok_or("original token claims")?;
    let original_claims: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded)?)?;
    let audience = original_claims
        .get("aud")
        .and_then(serde_json::Value::as_str)
        .ok_or("original audience")?;
    signed::sign(
        &json!({"alg":"EdDSA","typ":"at+jwt","kid":"stamp-a"}),
        &json!({"iss":signed::ISSUER,"aud":audience,"iat":decided_at.0,
            "exp":decided_at.0+3600,"grant_expires_at":grant_expires_at.0,
            "organization_id":signed::ORGANIZATION,"client_id":client_id,"azp":client_id,
            "principal":{"kind":"machine","installation_id":installation,"enrollment_id":enrollment_id}}),
        7,
    )
}

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
    reason = "Assertions compare real socket outcomes and owning rows with literal grant boundaries."
)]
#[tokio::test]
async fn renew_machine_token_extends_the_open_connection_and_a_refusal_keeps_its_expiry()
-> Result<()> {
    // Arrange: an ordinary client opens with the actual initial 600-second grant.
    let fixture = Fixture::new().await?;
    let (mut client, attempt, mut socket) = connect_client(&fixture, &fixture.token).await?;
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let connection = runtime
        .admit_machine_connection(&fixture.token)
        .await?
        .map_err(|_| "initial admission refused")?;
    assert_eq!(
        connection.effective_expires_at().0,
        signed::NOW_SECONDS + 600
    );

    // A token that fails sign-in verification is refused; the old expiry stands.
    send(
        &mut socket,
        &renew_frame("019f0000-0000-7000-8000-00000000c201", "not.a.token")?,
    )
    .await?;
    assert_eq!(
        renewal_result(&mut socket).await?,
        TokenRenewalResult::Refused {
            reason: TokenRenewalRefusal::TokenRejected
        }
    );

    // Act: at 300 s sign-in decides a fresh grant (read live), and the Router
    // sends the fresh token on the open connection (R40/V32, B3 renewal).
    fixture.clock.advance(Duration::from_secs(300))?;
    let grant = runtime.decide_machine_token(&serde_json::from_value(json!({
        "organizationId":signed::ORGANIZATION,"enrollmentId":connection.enrollment_id(),"clientId":connection.client_id(),
    }))?).await?;
    assert!(
        matches!(&grant, MachineTokenDecision::Grant { decided_at, grant_expires_at, .. }
        if decided_at.0 == signed::NOW_SECONDS + 300 && grant_expires_at.0 == signed::NOW_SECONDS + 900)
    );
    let token = renewed_token(&fixture.token, &grant, connection.client_id().as_str())?;
    send(
        &mut socket,
        &renew_frame("019f0000-0000-7000-8000-00000000c202", &token)?,
    )
    .await?;
    assert_eq!(
        renewal_result(&mut socket).await?,
        TokenRenewalResult::Renewed {
            effective_expires_at: chrono::DateTime::from_timestamp(signed::NOW_SECONDS + 900, 0)
                .ok_or("renewed expiry")?,
        }
    );

    // Assert: the same socket runs past its first token's expiry, at 600 and
    // 899 s, and is refused at 900 s, the grant boundary with no leeway.
    for (advance, request_id, name) in [
        (
            300,
            "019f0000-0000-7000-8000-00000000d102",
            "Renewed at 600",
        ),
        (
            299,
            "019f0000-0000-7000-8000-00000000d103",
            "Renewed at 899",
        ),
    ] {
        fixture.clock.advance(Duration::from_secs(advance))?;
        send(&mut socket, &LinkFrame::Ping).await?;
        assert_eq!(receive(&mut socket).await?, LinkFrame::Pong);
        let request = client.begin_request(caller()?, create_workspace(request_id, name)?)?;
        send(&mut socket, &request).await?;
        let (reply_id, reply) = workspace_reply(&mut client, &attempt, &mut socket).await?;
        assert_eq!(reply_id, correlation(&request)?);
        assert_eq!(created_workspace(&reply)?.name.as_str(), name);
    }
    fixture.clock.advance(Duration::from_secs(1))?;
    let expired = client.begin_request(
        caller()?,
        create_workspace("019f0000-0000-7000-8000-00000000d104", "Expired renewal")?,
    )?;
    send(&mut socket, &expired).await?;
    let (_, reply) = workspace_reply(&mut client, &attempt, &mut socket).await?;
    assert_eq!(
        reply.result,
        WorkspaceResult::Refused(RefusalReason::MachineRevoked)
    );
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT name FROM workspaces ORDER BY name",
            args: vec![],
        })
        .await?;
    assert_eq!(
        rows.rows,
        vec![
            vec![SqlValue::Text("Renewed at 600".into())],
            vec![SqlValue::Text("Renewed at 899".into())]
        ]
    );
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    fixture.close().await
}

fn renew_frame(correlation_id: &str, token: &str) -> Result<LinkFrame> {
    Ok(LinkFrame::Data(DataFrame::RenewMachineToken {
        correlation_id: correlation_id.parse()?,
        token: token.to_owned().try_into()?,
    }))
}

async fn renewal_result(socket: &mut WebSocket) -> Result<TokenRenewalResult> {
    match receive(socket).await? {
        LinkFrame::Data(DataFrame::MachineTokenRenewal { result, .. }) => Ok(result),
        other => Err(format!("expected a renewal answer, got {other:?}").into()),
    }
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
            sql: "SELECT name FROM workspaces ORDER BY name",
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
        return Err("workspace allowance fixture did not apply".into());
    }
    let write_id = "019f0000-0000-7000-8000-00000000d302";
    let payload = RequestPayload::Workspace(serde_json::from_value(json!({
        "requestId":write_id,"workspaceId":workspace.workspace_id,
        "operation":{"kind":"createArea","name":"Exactly one area"}
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
            sql: "SELECT request_id,answer FROM request_log",
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
                sql: "SELECT area_id,name FROM areas",
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
                sql: "SELECT area_id,name FROM areas",
                args: vec![]
            })
            .await?
            .rows,
        expected_area
    );
    assert_eq!(
        store
            .read(Statement {
                sql: "SELECT request_id,answer FROM request_log",
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
