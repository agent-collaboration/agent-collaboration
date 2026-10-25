//! A normal admitted session routes renewal to the real Registry rules.

use crate::{
    ConformanceClient, ConformanceResult,
    enrollment_scenarios::{enroll, response_peer},
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub async fn run_lease_renewal_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    let credential = "lease-renewal-v4";
    let expires_at: chrono::DateTime<chrono::Utc> = "2033-05-18T03:33:20Z".parse()?;
    let enrollment = json!({"kind":"enrollMachine","value":{
        "label":"renewal sender", "installation":"550e8400-e29b-41d4-a716-446655440099",
        "peer":"019f0000-0000-7000-8000-000000000495",
        "access_client_id":crate::access_client_id(credential)?,
        "credential_hash":format!("{:x}", Sha256::digest(credential.as_bytes())),
        "expires_at":expires_at
    }});
    let response = enroll(client, &enrollment).await?;
    response_peer(&response, expires_at)?;
    let mut socket = client
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
    let hello = json!({"kind":"hello","value":{"sessionId":"019f0000-0000-7000-8000-000000000490","schemaSupport":{"maxVersion":1}}});
    socket.send(Message::Text(hello.to_string())).await?;
    let expected_welcome = json!({"kind":"welcome","value":{"sessionId":"019f0000-0000-7000-8000-000000000490","databases":[]}});
    if receive(&mut socket).await? != expected_welcome {
        return Err("renewal fixture did not attach through the real Registry".into());
    }
    // No registration exists in this isolated central. The library's renewal
    // rule must return NotRegistered; an unrouted frame or socket close fails.
    for frame in [
        "019f0000-0000-7000-8000-000000000491",
        "019f0000-0000-7000-8000-000000000492",
    ] {
        let request = json!({"kind":"request","value":{"frame":frame,"body":{"kind":"lease","value":{"kind":"renew","value":{
            "attempt":"019f0000-0000-7000-8000-000000000493","database":"019f0000-0000-7000-8000-000000000494","epoch":1
        }}}}});
        socket.send(Message::Text(request.to_string())).await?;
        if receive(&mut socket).await?
            != json!({"kind":"answer","value":{"frame":frame,"body":{"kind":"lease","value":{"kind":"renew","value":{"kind":"refused","value":{"kind":"notRegistered"}}}}}})
        {
            return Err(
                "normal renewal did not return the Registry's correlated NotRegistered decision"
                    .into(),
            );
        }
    }
    socket.send(Message::Text(hello.to_string())).await?;
    if receive(&mut socket).await? != expected_welcome {
        return Err("renewal refusal made the admitted socket unusable".into());
    }
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    println!(
        "LEASE_CASE unregistered_renewal_and_repeat_are_correlated_and_keep_session_usable passed"
    );
    Ok(())
}

async fn receive(socket: &mut WebSocket) -> ConformanceResult<Value> {
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
        .await?
        .ok_or("session ended before the renewal answer")??;
    match frame {
        Message::Text(text) => Ok(serde_json::from_str(&text)?),
        Message::Binary(bytes) => Ok(serde_json::from_slice(&bytes)?),
        other => Err(format!("expected a correlated renewal response, got {other:?}").into()),
    }
}
