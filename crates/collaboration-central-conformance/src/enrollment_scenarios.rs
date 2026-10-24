//! Enrollment records the installation's peer without receiving a credential.

use crate::{ConformanceClient, ConformanceResult};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub async fn run_enrollment_identity_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    stable_peer_case(
        client,
        "019f0000-0000-7000-8000-000000000401",
        "019f0000-0000-7000-8000-000000000411",
        "enrollment-v7",
    )
    .await?;
    stable_peer_case(
        client,
        "550e8400-e29b-41d4-a716-446655440000",
        "019f0000-0000-7000-8000-000000000412",
        "enrollment-v4",
    )
    .await?;
    Ok(())
}

async fn stable_peer_case(
    client: &ConformanceClient,
    service: &str,
    proposed_peer: &str,
    credential: &str,
) -> ConformanceResult<()> {
    let expires_at: chrono::DateTime<chrono::Utc> = "2033-05-18T03:33:20Z".parse()?;
    // Exercise the HTTP decoder itself, including ServiceId v4. Constructing
    // the old v7-only InstallationId in a fixture would fail before HTTP.
    let request = json!({"kind":"enrollMachine","value":{
        "label":"enrollment sender", "installation":service, "peer":proposed_peer,
        "access_client_id":crate::access_client_id(credential)?,
        "credential_hash":format!("{:x}", Sha256::digest(credential.as_bytes())),
        "expires_at":expires_at
    }});

    let first = enroll(client, &request).await?;
    let peer = response_peer(&first, expires_at)?;
    if peer != uuid::Uuid::parse_str(proposed_peer)? {
        return Err("enrollment did not record the installation's supplied peer".into());
    }
    let repeat = enroll(client, &request).await?;
    if response_peer(&repeat, expires_at)? != peer {
        return Err("repeat enrollment changed the installation's peer".into());
    }
    let (left, right) = tokio::join!(enroll(client, &request), enroll(client, &request));
    if response_peer(&left?, expires_at)? != peer || response_peer(&right?, expires_at)? != peer {
        return Err("concurrent enrollment changed the installation's peer".into());
    }
    binding_refusals(client, &request).await?;
    println!("ENROLLMENT_CASE {credential}_service_gets_stable_v7_peer_and_exact_expiry passed");
    Ok(())
}

async fn binding_refusals(client: &ConformanceClient, request: &Value) -> ConformanceResult<()> {
    let mut competing = request.clone();
    *competing
        .pointer_mut("/value/installation")
        .ok_or("enrollment installation missing")? = json!("550e8400-e29b-41d4-a716-4466554400ff");
    *competing
        .pointer_mut("/value/access_client_id")
        .ok_or("enrollment Access client missing")? =
        json!(crate::access_client_id("peer-binding-conflict")?);
    *competing
        .pointer_mut("/value/credential_hash")
        .ok_or("enrollment credential hash missing")? = json!("distinct-peer-binding-hash");
    if enroll(client, &competing).await? != json!({"kind":"refused","value":"peerInUse"}) {
        return Err("another installation was not refused the existing peer".into());
    }
    let mut changed = request.clone();
    *changed
        .pointer_mut("/value/peer")
        .ok_or("enrollment peer missing")? = json!("019f0000-0000-7000-8000-000000000420");
    *changed
        .pointer_mut("/value/label")
        .ok_or("enrollment label missing")? = json!("must not replace the recorded identity");
    if enroll(client, &changed).await? != json!({"kind":"refused","value":"peerMismatch"}) {
        return Err("an existing installation was not refused a different peer".into());
    }
    for invalid in [json!("550e8400-e29b-41d4-a716-446655440000"), Value::Null] {
        let mut malformed = request.clone();
        *malformed
            .pointer_mut("/value/peer")
            .ok_or("enrollment peer missing")? = invalid;
        let response = client
            .with_owner_access(
                client
                    .client
                    .post(format!("{}/admin/v1/machines/enroll", client.base_url)),
            )?
            .json(&malformed)
            .send()
            .await?;
        if response.status() != reqwest::StatusCode::UNPROCESSABLE_ENTITY {
            return Err("native enrollment accepted a non-UUIDv7 peer".into());
        }
    }
    Ok(())
}

pub(crate) async fn enroll(
    client: &ConformanceClient,
    request: &Value,
) -> ConformanceResult<Value> {
    Ok(client
        .with_owner_access(
            client
                .client
                .post(format!("{}/admin/v1/machines/enroll", client.base_url)),
        )?
        .json(request)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

pub(crate) fn response_peer(
    response: &Value,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> ConformanceResult<uuid::Uuid> {
    if response.get("kind").and_then(Value::as_str) != Some("machineEnrolled") {
        return Err("enrollment did not return MachineEnrolled".into());
    }
    let value = response
        .get("value")
        .and_then(Value::as_object)
        .ok_or("enrollment omitted the registry peer and expiry")?;
    if value.len() != 4 {
        return Err(
            "enrollment must return exactly peer, expiry, authoritative receiver and relay identity".into(),
        );
    }
    if value.get("authoritativeReceiver").and_then(Value::as_str)
        != Some("019f0000-0000-7000-8000-000000000010")
    {
        return Err("enrollment did not return the configured authoritative receiver".into());
    }
    if value.get("relayPeer").and_then(Value::as_str)
        != Some("019f0000-0000-7000-8000-000000005240")
    {
        return Err("enrollment did not return the configured relay identity".into());
    }
    let peer = value
        .get("peer")
        .and_then(Value::as_str)
        .ok_or("enrollment omitted peer")?;
    let actual = value
        .get("expiresAt")
        .and_then(Value::as_str)
        .ok_or("enrollment omitted expiry")?
        .parse::<chrono::DateTime<chrono::Utc>>()?;
    if actual != expires_at {
        return Err("enrollment changed the requested expiry".into());
    }
    Ok(uuid::Uuid::parse_str(peer)?)
}

/// Normal Access + credential admission, followed by the shared Hello barrier.
pub async fn run_admitted_session_conformance<Observe, Observation>(
    client: &ConformanceClient,
    observe: Observe,
) -> ConformanceResult<()>
where
    Observe: Fn(uuid::Uuid) -> Observation,
    Observation: std::future::Future<Output = ConformanceResult<()>>,
{
    use futures_util::{SinkExt, StreamExt};
    use reqwest_websocket::{Message, RequestBuilderExt};
    let credential = "admitted-v4-session";
    let service = "550e8400-e29b-41d4-a716-446655440001";
    let expires_at: chrono::DateTime<chrono::Utc> = "2033-05-18T03:33:20Z".parse()?;
    let request = json!({"kind":"enrollMachine","value":{
        "label":"original admission label", "installation":service,
        "peer":"019f0000-0000-7000-8000-000000000451",
        "access_client_id":crate::access_client_id(credential)?,
        "credential_hash":format!("{:x}", Sha256::digest(credential.as_bytes())),
        "expires_at":expires_at
    }});
    let response = enroll(client, &request).await?;
    let peer = response_peer(&response, expires_at)?;
    if peer.get_version_num() != 7 || peer == uuid::Uuid::parse_str(service)? {
        return Err("session fixture did not get a distinct registry-owned peer".into());
    }
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
    let session = "019f0000-0000-7000-8000-000000000450";
    let hello = json!({"kind":"hello","value":{
        "sessionId":session,"schemaSupport":{"maxVersion":1}
    }});
    for turn in 0..2 {
        socket.send(Message::Text(hello.to_string())).await?;
        let message = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
            .await?
            .ok_or("admitted socket ended before Welcome")??;
        let frame: Value = match message {
            Message::Text(text) => serde_json::from_str(&text)?,
            Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
            other => {
                return Err(
                    format!("expected Welcome on admitted socket, received {other:?}").into(),
                );
            }
        };
        if frame != json!({"kind":"welcome","value":{"sessionId":session,"databases":[]}}) {
            return Err(format!("normal Hello returned {frame}").into());
        }
        observe(peer).await?;
        if turn == 0 {
            let mut relabeled = request.clone();
            let label = relabeled
                .get_mut("value")
                .and_then(|value| value.get_mut("label"))
                .ok_or("enrollment fixture omitted its label")?;
            *label = json!("later enrollment label");
            if response_peer(&enroll(client, &relabeled).await?, expires_at)? != peer {
                return Err("relabel changed the admitted peer".into());
            }
        }
    }
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    println!("ADMISSION_CASE v4_service_uses_distinct_peer_and_real_hello_barrier passed");
    Ok(())
}
