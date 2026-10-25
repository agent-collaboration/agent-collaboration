//! Normal authenticated connections replace one another within the same session.

use crate::{ConformanceClient, ConformanceResult, enroll_admitted_peer};
use collaboration_central_core::AccessRefusal;
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;

pub async fn run_session_replacement_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let service = "550e8400-e29b-41d4-a716-4466554400e1";
    let credential = "session-replacement-carryover";
    let session = "019f0000-0000-7000-8000-000000000921";
    let mut first = enroll_admitted_peer(
        client,
        service,
        "019f0000-0000-7000-8000-000000001009",
        credential,
        session,
    )
    .await?;
    let result = async {
        let owner = client
            .with_owner_access(client.client.get(format!("{}/v1/session", client.base_url)))?
            .upgrade()
            .send()
            .await?;
        if owner.status() != reqwest::StatusCode::UNAUTHORIZED
            || owner.into_inner().json::<AccessRefusal>().await? != AccessRefusal::WrongAudience
        {
            return Err("owner assertion unexpectedly admitted a machine socket".into());
        }
        let mut second = enroll_admitted_peer(client, service,"019f0000-0000-7000-8000-000000001009", credential, session).await?;
        let proof = async {
            if first.peer != second.peer {
                return Err("connection replacement changed the enrollment's stable peer".into());
            }
            if receive(&mut first.socket).await?
                != json!({"kind":"closed","value":{"reason":{"kind":"connectionReplaced"}}})
            {
                return Err("old connection did not receive typed ConnectionReplaced".into());
            }
            if !matches!(
                tokio::time::timeout(Duration::from_secs(5), first.socket.next()).await?,
                Some(Ok(Message::Close { .. })) | None
            ) {
                return Err("typed replacement was not followed by physical close".into());
            }
            second.socket.send(Message::Text(json!({"kind":"hello","value":{
                "sessionId":session,"schemaSupport":{"maxVersion":1}
            }}).to_string())).await?;
            if receive(&mut second.socket).await?
                != json!({"kind":"welcome","value":{"sessionId":session,"databases":[]}})
            {
                return Err("replacement socket did not retain its exact usable Welcome".into());
            }
            println!("SESSION_CARRYOVER_CASE authenticated_upgrade_exact_welcome_and_same_session_replacement passed");
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }.await;
        let closed = second.socket.close(reqwest_websocket::CloseCode::Normal, None).await;
        proof?;
        closed?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let _ = first
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    result
}

async fn receive(socket: &mut WebSocket) -> ConformanceResult<Value> {
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await?
        .ok_or("replacement socket ended before its data frame")??;
    match message {
        Message::Text(text) => Ok(serde_json::from_str(&text)?),
        Message::Binary(bytes) => Ok(serde_json::from_slice(&bytes)?),
        other => Err(format!("replacement expected a data frame, got {other:?}").into()),
    }
}
