//! Real public native links: signed admission, ordinary client identity and live frames.
mod alarm_rearm;
mod authority_races;
mod batch_contract;
mod fixture;
mod transport_contract;
use central_link_client::{ClientAction, ClientEvent, LinkClient};
use central_link_protocol::{DataFrame, LinkFrame, ReplyPayload, RequestPayload};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use fixture::{Fixture, INSTALLATION, OTHER_MACHINE, OTHER_ORG, receive, send};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt};
use serde_json::json;
use std::time::Duration;
use workspace_model::{
    Caller, RefusalReason, WorkspaceResult,
    central_mail::{CentralMailRefusal, CentralMailResult},
};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Socket and owning-store assertions must fail while fixture errors propagate."
)]
#[tokio::test]
async fn ordinary_client_uses_installation_and_receives_correlated_workspace_and_mail_replies()
-> Result<()> {
    // Arrange: the client knows only its installation and issued bearer token.
    let fixture = Fixture::new().await?;
    let mut client = LinkClient::new(INSTALLATION.parse()?, fixture.clock.clone());
    let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
        return Err("client did not connect".into());
    };
    let mut socket = fixture.connect().await?;

    // Act / Assert: a private central row key must never become the public identity.
    let welcome = receive(&mut socket).await?;
    assert_eq!(
        welcome,
        LinkFrame::Data(DataFrame::Welcome {
            machine_id: INSTALLATION.parse()?,
            central_time: chrono::DateTime::from_timestamp(signed::NOW_SECONDS, 0)
                .ok_or("fixture UTC")?,
        })
    );
    assert_eq!(client.receive(&attempt, welcome)?, ClientEvent::Welcomed);
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let rows = runtime
        .statement_host()
        .read(central_storage::Statement {
            sql: "SELECT machine_id FROM machines WHERE installation_id=?",
            args: vec![central_storage::SqlValue::Text(INSTALLATION.into())],
        })
        .await?;
    assert!(
        matches!(rows.rows.as_slice(), [row] if matches!(row.as_slice(), [central_storage::SqlValue::Text(id)] if id != INSTALLATION)),
        "fixture must keep private row and public installation distinct"
    );
    send(&mut socket, &LinkFrame::Ping).await?;
    assert_eq!(receive(&mut socket).await?, LinkFrame::Pong);
    let caller = Caller::Person {
        machine_id: INSTALLATION.parse()?,
        person_id: signed::OWNER.parse()?,
    };
    let create = RequestPayload::Workspace(serde_json::from_value(json!({
        "requestId":"019f0000-0000-7000-8000-00000000ce10","workspaceId":null,
        "operation":{"kind":"createWorkspace","name":"Real link workspace"},
    }))?);
    let request = client.begin_request(caller.clone(), create)?;
    let LinkFrame::Data(DataFrame::Request { request: expected }) = &request else {
        return Err("client request shape".into());
    };
    send(&mut socket, &request).await?;
    let reply = receive(&mut socket).await?;
    let ClientEvent::Reply {
        correlation_id,
        result: ReplyPayload::Workspace(reply),
    } = client.receive(&attempt, reply)?
    else {
        return Err("workspace reply missing".into());
    };
    assert_eq!(&correlation_id, expected.correlation_id());
    assert!(
        matches!(reply.result, WorkspaceResult::Done(workspace_model::WorkspaceAnswer::WorkspaceCreated { ref workspace }) if workspace.name.as_str() == "Real link workspace")
    );
    let mail = RequestPayload::Mail(serde_json::from_value(
        json!({"requestId":"019f0000-0000-7000-8000-00000000ce11","operation":{"kind":"setNoticeCadence","cadence":"m25"}}),
    )?);
    let request = client.begin_request(caller, mail)?;
    send(&mut socket, &request).await?;
    let reply = receive(&mut socket).await?;
    assert!(matches!(
        client.receive(&attempt, reply)?,
        ClientEvent::Reply {
            result: ReplyPayload::Mail(CentralMailResult::Refused(CentralMailRefusal::NotSession)),
            ..
        }
    ));
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Live refusal and heartbeat assertions observe the real public socket."
)]
#[tokio::test]
async fn semantic_requests_recheck_caller_and_original_grant_expiry() -> Result<()> {
    let fixture = Fixture::new().await?;
    let mut client = LinkClient::new(INSTALLATION.parse()?, fixture.clock.clone());
    let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
        return Err("client did not connect".into());
    };
    let mut socket = fixture.connect().await?;
    client.receive(&attempt, receive(&mut socket).await?)?;
    let payload = |request_id| -> Result<RequestPayload> {
        Ok(RequestPayload::Workspace(serde_json::from_value(json!({
            "requestId":request_id,"workspaceId":null,"operation":{"kind":"createWorkspace","name":"Must not create"},
        }))?))
    };
    let mismatched = Caller::Person {
        machine_id: OTHER_MACHINE.parse()?,
        person_id: signed::OWNER.parse()?,
    };
    send(
        &mut socket,
        &client.begin_request(mismatched, payload("019f0000-0000-7000-8000-00000000ce20")?)?,
    )
    .await?;
    let reply = receive(&mut socket).await?;
    assert!(matches!(
        client.receive(&attempt, reply)?,
        ClientEvent::Reply {
            result: ReplyPayload::Workspace(workspace_model::WorkspaceReply {
                result: WorkspaceResult::Refused(RefusalReason::MachineRevoked),
                ..
            }),
            ..
        }
    ));
    // Business time advances only through the host's injected clock.
    fixture.clock.advance(Duration::from_secs(600))?;
    send(&mut socket, &LinkFrame::Ping).await?;
    assert_eq!(receive(&mut socket).await?, LinkFrame::Pong);
    let caller = Caller::Person {
        machine_id: INSTALLATION.parse()?,
        person_id: signed::OWNER.parse()?,
    };
    send(
        &mut socket,
        &client.begin_request(caller, payload("019f0000-0000-7000-8000-00000000ce21")?)?,
    )
    .await?;
    let reply = receive(&mut socket).await?;
    assert!(matches!(
        client.receive(&attempt, reply)?,
        ClientEvent::Reply {
            result: ReplyPayload::Workspace(workspace_model::WorkspaceReply {
                result: WorkspaceResult::Refused(RefusalReason::MachineRevoked),
                ..
            }),
            ..
        }
    ));
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    assert!(
        runtime
            .statement_host()
            .read(central_storage::Statement {
                sql: "SELECT workspace_id FROM workspaces",
                args: vec![]
            })
            .await?
            .rows
            .is_empty()
    );
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP upgrade refusals must reject before any Welcome."
)]
#[tokio::test]
async fn public_route_requires_upgrade_and_header_token_and_matching_organization() -> Result<()> {
    let fixture = Fixture::new().await?;
    let client = fixture.client()?;
    assert!(
        !client
            .get(fixture.url(signed::ORGANIZATION))
            .bearer_auth(&fixture.token)
            .send()
            .await?
            .status()
            .is_success()
    );
    for request in [
        client.get(fixture.url(signed::ORGANIZATION)),
        client.get(format!(
            "{}?token={}",
            fixture.url(signed::ORGANIZATION),
            fixture.token
        )),
        client
            .get(fixture.url(signed::ORGANIZATION))
            .bearer_auth("bad-token"),
        client
            .get(fixture.url(signed::ORGANIZATION))
            // A well-formed installation with no machine row is refused.
            .bearer_auth(fixture.token_for_installation("019f0000-0000-7000-8000-00000000ce0f")?),
        client
            .get(fixture.url(OTHER_ORG))
            .bearer_auth(&fixture.token),
    ] {
        let response = request.upgrade().send().await?;
        assert_eq!(response.status().as_u16(), 401);
    }
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Unsupported application acknowledgements must close the real socket."
)]
#[tokio::test]
async fn acknowledgements_are_explicitly_unsupported_and_shutdown_drains_open_links() -> Result<()>
{
    let fixture = Fixture::new().await?;
    let mut socket = fixture.connect().await?;
    receive(&mut socket).await?;
    socket
        .send(Message::Text(serde_json::to_string(&DataFrame::Ack {
            message_id: "019f0000-0000-7000-8000-00000000ce30".parse()?,
        })?))
        .await?;
    assert!(matches!(
        socket.next().await,
        Some(Ok(Message::Close {
            code: reqwest_websocket::CloseCode::Unsupported,
            ..
        }))
    ));
    // Leave a second upgraded socket open: server shutdown must cancel and drain it.
    let mut open_socket = fixture.connect().await?;
    receive(&mut open_socket).await?;
    fixture.close().await?;
    assert!(!matches!(
        open_socket.next().await,
        Some(Ok(Message::Text(_)))
    ));
    Ok(())
}
