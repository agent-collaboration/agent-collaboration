//! Signed edge policy and revocation on the normal shared session route.

use crate::{
    ConformanceClient, ConformanceResult, FIXTURE_KID, access_client_id, enroll_admitted_peer,
    fixture_claims, machine_assertion, owner_assertion, sign_assertion,
};
use collaboration_central_core::{AccessCaller, AccessRefusal};
use fleet_sync_protocol::AuthRefusal;
use futures_util::StreamExt;
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::{Value, json};
use std::{sync::atomic::Ordering, time::Duration};

pub async fn run_shared_access_conformance(
    client: &ConformanceClient,
    jwks_requests: &std::sync::atomic::AtomicU64,
) -> ConformanceResult<()> {
    let service = "550e8400-e29b-41d4-a716-4466554400e7";
    let credential = "shared-access-machine";
    let other_credential = "shared-access-other-machine";
    let mut admitted = enroll_admitted_peer(
        client,
        service,
        "019f0000-0000-7000-8000-000000001005",
        credential,
        "019f0000-0000-7000-8000-000000000971",
    )
    .await?;
    let other = enroll_admitted_peer(
        client,
        "550e8400-e29b-41d4-a716-4466554400e8",
        "019f0000-0000-7000-8000-000000001006",
        other_credential,
        "019f0000-0000-7000-8000-000000000972",
    )
    .await?;
    let proof = async {
        let claims = fixture_claims(
            AccessCaller::Machine {
                client_id: access_client_id(credential)?,
            },
            false,
        );
        for (label, path, assertion, expected) in
            crate::access_scenarios::refused_assertions(claims.clone())?
        {
            let response = client
                .client
                .get(format!("{}{path}", client.base_url))
                .bearer_auth(credential)
                .header("Cf-Access-Jwt-Assertion", assertion)
                .send()
                .await?;
            if response.status() != reqwest::StatusCode::UNAUTHORIZED
                || response.json::<AccessRefusal>().await? != expected
            {
                return Err(format!("normal shared edge admitted the {label} assertion").into());
            }
        }
        let before = jwks_requests.load(Ordering::SeqCst);
        let unknown_key = client.client.get(format!("{}/v1/session", client.base_url))
            .bearer_auth(credential)
            .header("Cf-Access-Jwt-Assertion", sign_assertion(&claims, "unknown-access-key")?)
            .send().await?;
        if unknown_key.status() != reqwest::StatusCode::UNAUTHORIZED
            || unknown_key.json::<AccessRefusal>().await? != AccessRefusal::UnknownKey
            || jwks_requests.load(Ordering::SeqCst) != before + 1
        {
            return Err("unknown key was not refused after exactly one real JWKS refresh".into());
        }
        let missing = client.client.get(format!("{}/v1/session", client.base_url))
            .bearer_auth(credential).upgrade().send().await?;
        if missing.status() != reqwest::StatusCode::UNAUTHORIZED
            || missing.into_inner().json::<AccessRefusal>().await? != AccessRefusal::MissingAssertion
        {
            return Err("a missing Access assertion upgraded a real socket".into());
        }
        let mismatch = client.client.get(format!("{}/v1/session", client.base_url))
            .bearer_auth(other_credential)
            .header("Cf-Access-Jwt-Assertion", machine_assertion(credential)?)
            .upgrade().send().await?;
        let mismatch_status = mismatch.status();
        if mismatch_status != reqwest::StatusCode::FORBIDDEN {
            return Err(format!("cross-installation credential status was {mismatch_status}; expected403").into());
        }
        let mismatch_refusal = mismatch.into_inner().json::<AuthRefusal>().await?;
        if mismatch_refusal != AuthRefusal::WrongKind {
            return Err(format!("cross-installation credential refusal was {mismatch_refusal:?}; expectedWrongKind").into());
        }
        println!("ACCESS_SESSION_CASE signed_owner_machine_claim_refusals_refresh_missing_header_and_binding passed");
        let revoked = client.client.post(format!("{}/admin/v1/machines/revoke", client.base_url))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&json!({"kind":"revokeMachine","value":{"installation":service}}))
            .send().await?;
        if revoked.status() != reqwest::StatusCode::OK {
            return Err(format!("normal shared revoke route failed: {}", revoked.status()).into());
        }
        if revoked.json::<Value>().await? != json!({"kind":"machineRevoked"}) {
            return Err("revoke did not return its exact committed machine decision".into());
        }
        if receive(&mut admitted.socket).await? != json!({
            "kind":"closed","value":{"reason":{"kind":"credentialRejected","value":"revoked"}}
        }) {
            return Err("revocation did not send its typed live-session close".into());
        }
        if !matches!(tokio::time::timeout(Duration::from_secs(5), admitted.socket.next()).await?,
            Some(Ok(Message::Close { .. })) | None) {
            return Err("revocation did not physically close its old socket".into());
        }
        let denied = client.client.get(format!("{}/v1/session", client.base_url))
            .bearer_auth(credential)
            .header("Cf-Access-Jwt-Assertion", sign_assertion(&claims, FIXTURE_KID)?)
            .upgrade().send().await?;
        if denied.status() != reqwest::StatusCode::UNAUTHORIZED
            || denied.into_inner().json::<AuthRefusal>().await? != AuthRefusal::Revoked {
            return Err("a revoked enrollment reopened its session".into());
        }
        let unknown = client.client.get(format!("{}/not-a-central-route", client.base_url))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?).send().await?;
        if unknown.status() != reqwest::StatusCode::UNAUTHORIZED
            || unknown.json::<AccessRefusal>().await? != AccessRefusal::RouteRefused {
            return Err("an unknown route escaped the shared Access policy".into());
        }
        println!("ACCESS_SESSION_CASE revocation_closes_live_session_and_refuses_reupgrade passed");
        Ok::<_, Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = admitted
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let closed = other
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}

async fn receive(socket: &mut WebSocket) -> ConformanceResult<Value> {
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await?
        .ok_or("revoked socket ended before its typed close")??;
    match message {
        Message::Text(text) => Ok(serde_json::from_str(&text)?),
        Message::Binary(bytes) => Ok(serde_json::from_slice(&bytes)?),
        other => Err(format!("revocation expected a typed data close, got {other:?}").into()),
    }
}
