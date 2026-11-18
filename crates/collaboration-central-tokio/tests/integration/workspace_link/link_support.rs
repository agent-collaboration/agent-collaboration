//! Shared helpers for the native link tests: an ordinary client's connection,
//! workspace requests and replies, and a fresh machine token for a grant.
use super::fixture::{Fixture, INSTALLATION, receive};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use central_link_client::{ClientAction, ClientEvent, ConnectionAttempt, LinkClient};
use central_link_protocol::{CorrelationId, DataFrame, LinkFrame, ReplyPayload, RequestPayload};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use collaboration_central_core::identity_contract::MachineTokenDecision;
use reqwest_websocket::{RequestBuilderExt, WebSocket};
use serde_json::json;
use workspace_model::{Caller, Workspace, WorkspaceAnswer, WorkspaceReply, WorkspaceResult};

pub(super) fn caller() -> Result<Caller> {
    Ok(Caller::Person {
        machine_id: INSTALLATION.parse()?,
        person_id: signed::OWNER.parse()?,
    })
}

pub(super) fn create_workspace(request_id: &str, name: &str) -> Result<RequestPayload> {
    Ok(RequestPayload::Workspace(serde_json::from_value(json!({
        "requestId":request_id,"workspaceId":null,
        "operation":{"kind":"createWorkspace","name":name}
    }))?))
}

pub(super) fn correlation(frame: &LinkFrame) -> Result<CorrelationId> {
    let LinkFrame::Data(DataFrame::Request { request }) = frame else {
        return Err("client did not produce a Request".into());
    };
    Ok(request.correlation_id().clone())
}

pub(super) async fn connect_client(
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

pub(super) async fn workspace_reply(
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

pub(super) fn created_workspace(reply: &WorkspaceReply) -> Result<Workspace> {
    let WorkspaceResult::Done(WorkspaceAnswer::WorkspaceCreated { workspace }) = &reply.result
    else {
        return Err(format!("workspace creation did not succeed: {:?}", reply.result).into());
    };
    Ok(workspace.clone())
}

pub(super) fn renewed_token(
    original: &str,
    grant: &MachineTokenDecision,
    client_id: &str,
) -> Result<String> {
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
