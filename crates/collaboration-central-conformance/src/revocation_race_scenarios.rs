//! Hold real delivery, then readmit and replay the old recorded call.

use crate::{ConformanceClient, ConformanceResult, enroll_admitted_peer, owner_assertion};
use collaboration_central_core::{
    RevocationControl, RevocationControlInput, RevocationObservation, RevocationWait,
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::task::JoinSet;

const SERVICE: &str = "550e8400-e29b-41d4-a716-4466554400e9";
const CREDENTIAL: &str = "revocation-readmission-race";
const OLD_SESSION: &str = "019f0000-0000-7000-8000-000000000981";
const NEW_SESSION: &str = "019f0000-0000-7000-8000-000000000982";

pub async fn run_revocation_race_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut old = enroll_admitted_peer(
        client,
        SERVICE,
        "019f0000-0000-7000-8000-000000001008",
        CREDENTIAL,
        OLD_SESSION,
    )
    .await?;
    let mut requests = JoinSet::new();
    let proof = async {
        let before = control(client, RevocationControl::Observe).await?;
        let initial = before.enrollment.ok_or("revocation fixture enrollment missing")?;
        if initial.peer.as_uuid() != old.peer || initial.last_revocation_operation.is_some() || before.operation.is_some() {
            return Err("fresh enrollment has an unexpected revocation history".into());
        }
        control(client, RevocationControl::ArmDelivery).await?;
        let caller = client.clone();
        requests.spawn(async move {
            Ok::<Value, Box<dyn std::error::Error+Send+Sync>>(caller.client
                .post(format!("{}/admin/v1/machines/revoke", caller.base_url))
                .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
                .json(&json!({"kind":"revokeMachine","value":{"installation":SERVICE}}))
                .send().await?.error_for_status()?.json().await?)
        });
        let held = observed(client, RevocationWait::DeliveryGateReached, |value| {
            value.gate_armed && value.gate_reached
                && value.enrollment.as_ref().and_then(|row| row.last_revocation_operation)
                    .is_some_and(|operation| value.calls.iter().any(|call| call.operation == operation))
        }).await?;
        let enrollment = held.enrollment.ok_or("held revoke lost its enrollment")?;
        if !matches!(held.operation.as_ref().map(|operation| &operation.state), Some(durable_steps::OperationState::Running { .. }))
            || enrollment.last_revocation_operation.is_none()
            || enrollment.fingerprint == initial.fingerprint {
            return Err("delivery was not held after actual atomic revoke admission".into());
        }
        let old_call = held.calls.iter()
            .find(|call| Some(call.operation) == enrollment.last_revocation_operation)
            .cloned().ok_or("held revocation has no actual CallId")?;
        let proposed = json!({"kind":"enrollMachine","value":{
            "installation":SERVICE, "peer":old.peer, "label":"must not be written during revoke",
            "access_client_id":crate::access_client_id(CREDENTIAL)?,
            "credential_hash":format!("{:x}", Sha256::digest(CREDENTIAL.as_bytes())),
            "expires_at":"2033-05-18T03:33:20Z"
        }});
        let answer = crate::enrollment_scenarios::enroll(client, &proposed).await?;
        if answer != json!({"kind":"revocationInProgress"}) {
            // Keep the causal observation in an early-admission wrong build:
            // the new session is fully attached while delivery is still held.
            if answer.get("kind").and_then(Value::as_str) == Some("machineEnrolled") {
                let mut premature = enroll_admitted_peer(client, SERVICE,"019f0000-0000-7000-8000-000000001008", CREDENTIAL, NEW_SESSION).await?;
                control(client, RevocationControl::ReleaseDelivery).await?;
                let ended = receive(&mut premature.socket).await;
                let _ = premature.socket.close(reqwest_websocket::CloseCode::Normal, None).await;
                if ended? == revoked_close() {
                    return Err("wrong early readmission ended the new session when held revocation arrived".into());
                }
            }
            return Err(format!("pending revocation admitted readmission: {answer}").into());
        }
        if control(client, RevocationControl::Observe).await?.enrollment != Some(enrollment) {
            return Err("refused readmission changed the owning enrollment row".into());
        }
        println!("REVOCATION_CASE held_delivery_refuses_readmission_without_changing_enrollment passed");
        // The authenticated old session keeps routing until EndSessions commits.
        live(&mut old.socket, OLD_SESSION).await?;
        control(client, RevocationControl::ReleaseDelivery).await?;
        let answer = tokio::time::timeout(Duration::from_secs(10), requests.join_next()).await?
            .ok_or("revoke request disappeared")???;
        if answer != json!({"kind":"machineRevoked"}) || receive(&mut old.socket).await? != revoked_close() {
            return Err("released revocation did not close its actual old session".into());
        }
        observed(client, RevocationWait::OperationCompleted, |value| matches!(value.operation.as_ref().map(|operation| &operation.state), Some(durable_steps::OperationState::Completed { .. }))).await?;
        // Readmission may answer pending after admitting the mailbox Open.
        // Observe its owning journal without an admin retry driving it, then
        // repeat the exact active contents while opening the new session.
        let readmit = json!({"kind":"enrollMachine","value":{
            "installation":SERVICE,"peer":old.peer,"label":CREDENTIAL,
            "access_client_id":crate::access_client_id(CREDENTIAL)?,
            "credential_hash":format!("{:x}",Sha256::digest(CREDENTIAL.as_bytes())),
            "expires_at":"2033-05-18T03:33:20Z"
        }});
        let answer = crate::enrollment_scenarios::enroll(client, &readmit).await?;
        if answer != json!({"kind":"enrollmentInProgress"})
            && answer.get("kind").and_then(Value::as_str) != Some("machineEnrolled") {
            return Err(format!("unexpected readmission answer: {answer}").into());
        }
        observed(client, RevocationWait::ReadmissionCompleted, |value| {
            value.enrollment.as_ref().is_some_and(|row| !row.revoked)
                && matches!(value.readmission_operation.as_ref().map(|operation| &operation.state),
                    Some(durable_steps::OperationState::Completed { .. }))
        }).await?;
        let mut readmitted = enroll_admitted_peer(client, SERVICE,"019f0000-0000-7000-8000-000000001008", CREDENTIAL, NEW_SESSION).await?;
        let survival = async {
            if readmitted.peer != old.peer { return Err("readmission minted another PeerId".into()); }
            control(client, RevocationControl::Replay { call: old_call }).await?;
            live(&mut readmitted.socket, NEW_SESSION).await?;
            // A revoked session remains ended. Reenrollment must not erase its
            // durable PeerRevoked row merely to admit the fresh session.
            let mut stale = client.with_machine_access(client.client.get(format!("{}/v1/session", client.base_url)).bearer_auth(CREDENTIAL), CREDENTIAL)?
                .upgrade().send().await?.into_websocket().await?;
            let stale_proof = async {
                stale.send(Message::Text(hello(OLD_SESSION).to_string())).await?;
                if receive(&mut stale).await? != json!({"kind":"closed","value":{"reason":{"kind":"sessionEnded"}}}) {
                    return Err("readmission reset the old revoked session's terminal row".into());
                }
                live(&mut readmitted.socket, NEW_SESSION).await?;
                Ok::<_, Box<dyn std::error::Error+Send+Sync>>(())
            }.await;
            let _ = stale.close(reqwest_websocket::CloseCode::Normal, None).await;
            stale_proof?;
            println!("REVOCATION_CASE readmitted_session_survives_late_old_call_and_old_session_stays_ended passed");
            Ok::<_, Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        let closed = readmitted.socket.close(reqwest_websocket::CloseCode::Normal, None).await;
        survival?; closed?;
        Ok::<_, Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = control(client, RevocationControl::ReleaseDelivery).await;
    requests.abort_all();
    while requests.join_next().await.is_some() {}
    let _ = old
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof
}

async fn control(
    client: &ConformanceClient,
    command: RevocationControl,
) -> ConformanceResult<RevocationObservation> {
    Ok(client
        .with_owner_access(client.client.post(format!(
            "{}/admin/v1/conformance/revocation",
            client.base_url
        )))?
        .json(&RevocationControlInput {
            service: SERVICE.to_owned().try_into()?,
            control: command,
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
async fn observed(
    client: &ConformanceClient,
    condition: RevocationWait,
    ready: impl FnOnce(&RevocationObservation) -> bool,
) -> ConformanceResult<RevocationObservation> {
    let value = tokio::time::timeout(
        Duration::from_secs(8),
        control(client, RevocationControl::Await { condition }),
    )
    .await??;
    if !ready(&value) {
        return Err(
            format!("revocation wait {condition:?} answered before its owning state").into(),
        );
    }
    Ok(value)
}
fn revoked_close() -> Value {
    json!({"kind":"closed","value":{"reason":{"kind":"credentialRejected","value":"revoked"}}})
}
fn hello(session: &str) -> Value {
    json!({"kind":"hello","value":{"sessionId":session,"schemaSupport":{"maxVersion":1}}})
}
pub(crate) async fn live(socket: &mut WebSocket, session: &str) -> ConformanceResult<()> {
    socket
        .send(Message::Text(hello(session).to_string()))
        .await?;
    let actual = receive(socket)
        .await
        .map_err(|error| format!("live session {session} did not answer Hello: {error}"))?;
    if actual != json!({"kind":"welcome","value":{"sessionId":session,"databases":[]}}) {
        return Err(
            format!("a late revocation replay ended the readmitted session: {actual}").into(),
        );
    }
    Ok(())
}
pub(crate) async fn receive(socket: &mut WebSocket) -> ConformanceResult<Value> {
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await?
        .ok_or("revocation race socket ended before its data frame")??;
    match message {
        Message::Text(text) => Ok(serde_json::from_str(&text)?),
        Message::Binary(bytes) => Ok(serde_json::from_slice(&bytes)?),
        other => Err(format!("expected revocation race data frame, got {other:?}").into()),
    }
}
