//! A journaled Registry call reaches a different enrolled holder over its real socket.

use crate::{
    ConformanceClient, ConformanceResult,
    enrollment_scenarios::{enroll, response_peer},
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub struct AdmittedPeerSocket {
    pub peer: uuid::Uuid,
    pub socket: WebSocket,
    hello: Value,
    welcome: Value,
}
pub async fn enroll_admitted_peer(
    client: &ConformanceClient,
    service: &str,
    proposed_peer: &str,
    credential: &str,
    session: &str,
) -> ConformanceResult<AdmittedPeerSocket> {
    enroll_admitted_peer_with_label(
        client,
        service,
        proposed_peer,
        credential,
        credential,
        session,
    )
    .await
}

pub async fn enroll_admitted_peer_with_label(
    client: &ConformanceClient,
    service: &str,
    proposed_peer: &str,
    credential: &str,
    label: &str,
    session: &str,
) -> ConformanceResult<AdmittedPeerSocket> {
    let mut admitted =
        enroll_open_peer_with_label(client, service, proposed_peer, credential, label, session)
            .await?;
    admitted
        .socket
        .send(Message::Text(admitted.hello.to_string()))
        .await?;
    if receive(&mut admitted.socket).await? != admitted.welcome {
        return Err("holder fixture did not complete normal admission".into());
    }
    Ok(admitted)
}

pub async fn enroll_open_peer_with_label(
    client: &ConformanceClient,
    service: &str,
    proposed_peer: &str,
    credential: &str,
    label: &str,
    session: &str,
) -> ConformanceResult<AdmittedPeerSocket> {
    let expires_at: chrono::DateTime<chrono::Utc> = "2033-05-18T03:33:20Z".parse()?;
    let response = enroll(client, &json!({"kind":"enrollMachine","value":{
        "label":label,"installation":service,"peer":proposed_peer,"access_client_id":crate::access_client_id(credential)?,
        "credential_hash":format!("{:x}", Sha256::digest(credential.as_bytes())),"expires_at":expires_at
    }})).await?;
    let peer = response_peer(&response, expires_at)?;
    if peer != uuid::Uuid::parse_str(proposed_peer)? {
        return Err("holder enrollment changed the supplied installation peer".into());
    }
    let socket = client
        .with_machine_access(
            client
                .client
                .get(format!("{}/v1/session", client.base_url))
                .bearer_auth(credential),
            credential,
        )?
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    let hello =
        json!({"kind":"hello","value":{"sessionId":session,"schemaSupport":{"maxVersion":1}}});
    let welcome = json!({"kind":"welcome","value":{"sessionId":session,"databases":[]}});
    Ok(AdmittedPeerSocket {
        peer,
        socket,
        hello,
        welcome,
    })
}
async fn receive(socket: &mut WebSocket) -> ConformanceResult<Value> {
    let message = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
        .await
        .map_err(|_| "holder socket did not produce its next frame within the fixture bound")?
        .ok_or("holder socket ended without its request")??;
    match message {
        Message::Text(text) => Ok(serde_json::from_str(&text)?),
        Message::Binary(bytes) => Ok(serde_json::from_slice(&bytes)?),
        other => Err(format!("expected a holder data frame, received {other:?}").into()),
    }
}
async fn invoke(client: &ConformanceClient, input: &Value) -> ConformanceResult<Value> {
    Ok(client
        .with_owner_access(client.client.post(format!(
            "{}/admin/v1/conformance/holder-call",
            client.base_url
        )))?
        .json(input)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

pub async fn run_holder_reply_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut caller = enroll_admitted_peer(
        client,
        "550e8400-e29b-41d4-a716-4466554400a1",
        "019f0000-0000-7000-8000-000000001002",
        "holder-caller",
        "019f0000-0000-7000-8000-000000000501",
    )
    .await?;
    let mut holder = enroll_admitted_peer(
        client,
        "550e8400-e29b-41d4-a716-4466554400b2",
        "019f0000-0000-7000-8000-000000001003",
        "holder-target",
        "019f0000-0000-7000-8000-000000000502",
    )
    .await?;
    if caller.peer == holder.peer {
        return Err("caller and holder must be distinct enrollments".into());
    }
    let database = "019f0000-0000-7000-8000-000000000503";
    let transfer = "019f0000-0000-7000-8000-000000000504";
    for (attempt, request_kind, wire_kind, answer) in [
        (
            "019f0000-0000-7000-8000-000000000505",
            "release",
            "release",
            json!({"kind":"released","value":{"final":{"epoch":1,"sequence":0}}}),
        ),
        (
            "019f0000-0000-7000-8000-000000000506",
            "withdrawal",
            "withdrawRelease",
            json!({"kind":"withdrawn"}),
        ),
    ] {
        let request = json!({"database":database,"epoch":1,"transfer":transfer});
        let input = json!({"attempt":attempt,"holder":holder.peer,"request":{"kind":request_kind,"value":request}});
        let http = client.client.clone();
        let url = format!("{}/admin/v1/conformance/holder-call", client.base_url);
        let assertion = crate::owner_assertion()?;
        let sent = input.clone();
        let mut pending = tokio::spawn(async move {
            Ok::<Value, Box<dyn std::error::Error + Send + Sync>>(
                http.post(url)
                    .header("Cf-Access-Jwt-Assertion", assertion)
                    .json(&sent)
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?,
            )
        });
        let mut settled = false;
        let result = async {
            let mut early = Value::Null;
            let frame = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    tokio::select! {
                        frame = receive(&mut holder.socket) => return frame,
                        reply = &mut pending, if !settled => {
                            settled = true;
                            early = reply??;
                            if early.get("kind").and_then(Value::as_str) != Some("pending") { return Err(format!("holder call completed before receiving its socket answer: {early}").into()); }
                        }
                    }
                }
            }).await.map_err(|_| format!("holder request was not delivered; last caller journal: {early}"))??;
            let frame_id = frame.get("value").and_then(|value| value.get("frame")).and_then(Value::as_str).ok_or("holder request omitted frame identity")?;
            let call = frame.pointer("/value/body/value/value/call").cloned().ok_or("holder request omitted durable CallId")?;
            if frame.get("kind").and_then(Value::as_str) != Some("request")
                || frame.pointer("/value/body/kind").and_then(Value::as_str) != Some("crate")
                || frame.pointer("/value/body/value/kind").and_then(Value::as_str) != Some(wire_kind)
                || frame.pointer("/value/body/value/value/request") != Some(&request)
                || call.get("step").and_then(Value::as_str) != Some("holder_request") {
                return Err(format!("wrong holder request: {frame}").into());
            }
            let operation = call.get("operation").and_then(Value::as_str).ok_or("holder call operation missing")?;
            if uuid::Uuid::parse_str(operation)?.get_version_num() != 7 { return Err("holder call operation is not v7".into()); }
            let answer_frame = json!({"kind":"answer","value":{"frame":frame_id,"body":{"kind":"crate","value":{"kind":wire_kind,"value":answer}}}});
            holder.socket.send(Message::Text(answer_frame.to_string())).await?;
            if !settled { let _ = (&mut pending).await??; settled = true; }
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    let reply = invoke(client, &input).await?;
                    if reply == json!({"kind":"completed","value":{"kind":request_kind,"value":answer}}) { return Ok::<(), Box<dyn std::error::Error + Send + Sync>>(()); }
                    if reply.get("kind").and_then(Value::as_str) != Some("pending") { return Err(format!("holder answer was not correlated: {reply}").into()); }
                    tokio::task::yield_now().await;
                }
            }).await.map_err(|_| format!("holder {request_kind} answer did not complete its real caller journal"))??;
            // An ordered application barrier detects a request misdelivered
            // to the other enrolled socket, without an absence-by-sleep check.
            caller.socket.send(Message::Text(caller.hello.to_string())).await?;
            if receive(&mut caller.socket).await? != caller.welcome { return Err("holder request reached the other enrollment".into()); }
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }.await;
        if !pending.is_finished() {
            pending.abort();
            let _ = pending.await;
        }
        result?;
    }
    caller
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    holder
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    println!(
        "HOLDER_CASE registry_maps_distinct_target_and_correlates_release_and_withdrawal_socket_answers passed"
    );
    Ok(())
}
