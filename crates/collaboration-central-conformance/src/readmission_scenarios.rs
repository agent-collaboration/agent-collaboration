//! Real mailbox Open and activation interleavings through normal admin and sessions.

use crate::revocation_race_scenarios::{live, receive};
use crate::{ConformanceClient, ConformanceResult, enroll_admitted_peer};
use collaboration_central_core::{
    RevocationControl, RevocationControlInput, RevocationObservation,
};
use durable_steps::{OperationState, storage::StepState};
use futures_util::StreamExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::task::JoinSet;

const SERVICE: &str = "550e8400-e29b-41d4-a716-4466554400e7";
const OLD_CREDENTIAL: &str = "activation-before-finish-old";
const NEW_CREDENTIAL: &str = "activation-before-finish-new";

pub async fn run_readmission_open_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut active = enroll_admitted_peer(
        client,
        SERVICE,
        "019f0000-0000-7000-8000-000000001007",
        OLD_CREDENTIAL,
        "019f0000-0000-7000-8000-000000000971",
    )
    .await?;
    let stable_peer = active.peer;
    let mut requests = JoinSet::new();
    let mut old_opens: Vec<durable_steps::CallId> = Vec::new();
    let mut old_closes: Vec<durable_steps::CallId> = Vec::new();
    let mut old_ends: Vec<durable_steps::CallId> = Vec::new();
    let proof = async {
        for (credential, session) in [
            (NEW_CREDENTIAL, "019f0000-0000-7000-8000-000000000972"),
            ("readmission-second-cycle", "019f0000-0000-7000-8000-000000000973"),
        ] {
            revoke(client).await?;
            let closed = observed(client, |value| completed(value.operation.as_ref())).await?;
            if receive(&mut active.socket).await? != json!({"kind":"closed","value":{"reason":{"kind":"credentialRejected","value":"revoked"}}}) {
                return Err("a completed revoke did not close its old session".into());
            }
            let revoke = closed.enrollment.as_ref().and_then(|row| row.last_revocation_operation)
                .ok_or("completed revoke has no owning operation")?;
            old_closes.push(closed.close_calls.iter().find(|call| call.operation == revoke)
                .cloned().ok_or("completed revoke has no physical Close call")?);
            old_ends.push(closed.calls.iter().find(|call| call.operation == revoke)
                .cloned().ok_or("completed revoke has no EndSessions call")?);
            for call in &old_opens {
                control(client, RevocationControl::Replay { call: call.clone() }).await?;
            }
            control(client, RevocationControl::ArmOpen).await?;
            let proposed = json!({"kind":"enrollMachine","value":{
                "installation":SERVICE,"peer":stable_peer,"label":credential,
                "access_client_id":crate::access_client_id(credential)?,
                "credential_hash":format!("{:x}", Sha256::digest(credential.as_bytes())),
                "expires_at":"2033-05-18T03:33:20Z"
            }});
            let caller = client.clone();
            let request = proposed.clone();
            requests.spawn(async move { crate::enrollment_scenarios::enroll(&caller, &request).await });
            let held = observed(client, |value| value.open_pause.reached).await?;
            let reserved = held.enrollment.clone().ok_or("held Open lost the reserved enrollment")?;
            if !reserved.revoked || reserved.peer.as_uuid() != stable_peer {
                return Err("delayed Open activated the row before the mailbox answered".into());
            }
            let enrollment = reserved.last_enrollment_operation.ok_or("held Open lacks enrollment operation")?;
            old_opens.push(held.open_calls.iter().find(|call| call.operation == enrollment)
                .cloned().ok_or("held Open has no actual durable call")?);
            let response = client.with_machine_access(client.client.get(format!("{}/v1/session",client.base_url))
                .bearer_auth(credential), credential)?.send().await?;
            if response.status() != reqwest::StatusCode::UNAUTHORIZED || response.json::<Value>().await? != json!("revoked") {
                return Err("reserved credential authenticated before Open completed".into());
            }
            for request in [proposed.clone(), {
                let mut overlap = proposed;
                *overlap.pointer_mut("/value/label").ok_or("overlap enrollment label missing")? = json!("must not overwrite reserved identities");
                overlap
            }] {
                if crate::enrollment_scenarios::enroll(client, &request).await? != json!({"kind":"enrollmentInProgress"}) {
                    return Err("overlapping enrollment was admitted while Open was held".into());
                }
            }
            if revoke_answer(client).await? != json!({"kind":"enrollmentInProgress"})
                || control(client, RevocationControl::Observe).await?.enrollment.as_ref() != Some(&reserved) {
                return Err("pending revoke or overlap changed the reserved row".into());
            }
            control(client, RevocationControl::ArmBeforeActivation).await?;
            control(client, RevocationControl::ReleaseOpen).await?;
            let opened = observed(client, |value| value.before_activation_pause.reached).await?;
            let saved_open = opened.open_step.as_ref().ok_or("Open's saved answer missing before activation")?;
            if saved_open.name.as_str() != "open_mailbox"
                || !matches!(&saved_open.state, StepState::CallDone { .. })
                || opened.activation_step.is_some()
                || opened.enrollment.as_ref() != Some(&reserved) {
                return Err(format!("after-Open checkpoint changed credentials or activated early: {opened:?}").into());
            }
            let response = client.with_machine_access(client.client.get(format!("{}/v1/session",client.base_url))
                .bearer_auth(credential), credential)?.send().await?;
            if response.status() != reqwest::StatusCode::UNAUTHORIZED || response.json::<Value>().await? != json!("revoked") {
                return Err("reserved credential authenticated after Open but before activation".into());
            }
            println!("READMISSION_CASE saved_open_before_activation_keeps_reserved_credential_inactive passed");
            control(client, RevocationControl::ReleaseBeforeActivation).await?;
            observed(client, |value| value.enrollment.as_ref().is_some_and(|row| !row.revoked)
                && value.readmission_operation.as_ref().is_some_and(|record| record.id == enrollment)
                && completed(value.readmission_operation.as_ref())).await?;
            let next = enroll_admitted_peer(client, SERVICE,"019f0000-0000-7000-8000-000000001007", credential, session).await?;
            if next.peer != stable_peer { return Err("a later readmission changed PeerId".into()); }
            let previous = std::mem::replace(&mut active, next);
            let _ = previous.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
            for call in old_closes.iter().chain(&old_ends) {
                control(client, RevocationControl::Replay { call: call.clone() }).await?;
            }
            live(&mut active.socket, session).await?;
            let answer = requests.join_next().await.ok_or("readmission caller disappeared")???;
            if answer != json!({"kind":"enrollmentInProgress"}) && answer.get("kind").and_then(Value::as_str) != Some("machineEnrolled") {
                return Err(format!("unexpected readmission answer {answer}").into());
            }
        }
        println!("READMISSION_CASE two_cycles_hold_open_reserve_credentials_and_replay_old_calls passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = control(client, RevocationControl::ReleaseOpen).await;
    let _ = control(client, RevocationControl::ReleaseBeforeActivation).await;
    requests.abort_all();
    while requests.join_next().await.is_some() {}
    let _ = active
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof
}

pub async fn run_readmission_activation_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let old = enroll_admitted_peer(
        client,
        SERVICE,
        "019f0000-0000-7000-8000-000000001007",
        OLD_CREDENTIAL,
        "019f0000-0000-7000-8000-000000000971",
    )
    .await?;
    let mut requests = JoinSet::new();
    let proof = async {
        revoke(client).await?;
        observed(client, |value| completed(value.operation.as_ref())).await?;
        control(client, RevocationControl::ArmActivation).await?;
        let proposed = json!({"kind":"enrollMachine","value":{
            "installation":SERVICE,"peer":old.peer,"label":NEW_CREDENTIAL,
            "access_client_id":crate::access_client_id(NEW_CREDENTIAL)?,
            "credential_hash":format!("{:x}",Sha256::digest(NEW_CREDENTIAL.as_bytes())),
            "expires_at":"2033-05-18T03:33:20Z"
        }});
        let caller = client.clone();
        let request = proposed.clone();
        requests.spawn(async move { crate::enrollment_scenarios::enroll(&caller, &request).await });

        // No admin retry drives this operation. The checkpoint defers operation
        // completion through a conformance retry after its real local receipt;
        // the normal due-work scheduler stays free to process the later revoke.
        let held = observed(client, |value| value.activation_pause.reached).await?;
        let active = held.enrollment.as_ref().ok_or("activated row missing")?;
        if active.revoked || active.peer.as_uuid() != old.peer
            || !matches!(held.readmission_operation.as_ref().map(|record| &record.state), Some(OperationState::Running { .. })) {
            return Err("activation pause did not retain a real active row with unfinished operation".into());
        }
        let saved = held.activation_step.clone().ok_or("activation's committed local receipt missing")?;
        if saved.name.as_str() != "activate_enrollment" || !matches!(&saved.state, StepState::Local { output } if output == b"null") {
            return Err("activation pause preceded its actual saved local receipt".into());
        }
        let mut overlap = proposed;
        *overlap.pointer_mut("/value/label").ok_or("overlap enrollment label missing")? = json!("overlapping reservation must be refused");
        if crate::enrollment_scenarios::enroll(client, &overlap).await? != json!({"kind":"enrollmentInProgress"})
            || control(client, RevocationControl::Observe).await?.enrollment.as_ref() != Some(active) {
            return Err("unfinished activation accepted overlapping enrollment changes".into());
        }
        // This repeats the exact active contents and authenticates the newly
        // reserved credential through the normal Access/credential/session edge.
        let mut current = enroll_admitted_peer(client, SERVICE,"019f0000-0000-7000-8000-000000001007", NEW_CREDENTIAL,
            "019f0000-0000-7000-8000-000000000972").await?;
        let survival = async {
            revoke(client).await?;
            observed(client, |value| completed(value.operation.as_ref())).await?;
            let message = tokio::time::timeout(Duration::from_secs(5), current.socket.next()).await?
                .ok_or("new session ended without revocation frame")??;
            let actual: Value = match message {
                reqwest_websocket::Message::Text(text) => serde_json::from_str(&text)?,
                reqwest_websocket::Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
                other => return Err(format!("revocation returned {other:?}").into()),
            };
            if actual != json!({"kind":"closed","value":{"reason":{"kind":"credentialRejected","value":"revoked"}}}) {
                return Err(format!("active unfinished enrollment was not revoked at once: {actual}").into());
            }
            let revoked = control(client, RevocationControl::Observe).await?;
            if !revoked.enrollment.as_ref().is_some_and(|row| row.revoked)
                || revoked.activation_step.as_ref() != Some(&saved) {
                return Err("revoke changed the saved activation receipt or failed to revoke".into());
            }
            control(client, RevocationControl::ReleaseActivation).await?;
            let finished = observed(client, |value| completed(value.readmission_operation.as_ref())).await?;
            // finish deletes step rows; only the unfinished operation retains
            // its local receipts. The durable result here is the revoked row.
            if !finished.enrollment.as_ref().is_some_and(|row| row.revoked) {
                return Err("finishing the old enrollment reactivated the revoked row".into());
            }
            let response = client.with_machine_access(client.client.get(format!("{}/v1/session",client.base_url))
                .bearer_auth(NEW_CREDENTIAL), NEW_CREDENTIAL)?.send().await?;
            if response.status() != reqwest::StatusCode::UNAUTHORIZED || response.json::<Value>().await? != json!("revoked") {
                return Err("old activation replay made the revoked credential usable".into());
            }
            println!("READMISSION_CASE activation_receipt_survives_revoke_before_engine_finish passed");
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        let _ = current.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        survival?;
        if let Some(result) = requests.join_next().await {
            let answer = result??;
            if answer != json!({"kind":"enrollmentInProgress"}) && answer.get("kind").and_then(Value::as_str) != Some("machineEnrolled") {
                return Err(format!("unexpected original readmission answer {answer}").into());
            }
        } else { return Err("original readmission caller disappeared".into()); }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = control(client, RevocationControl::ReleaseActivation).await;
    requests.abort_all();
    while requests.join_next().await.is_some() {}
    let _ = old
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof
}

fn completed(value: Option<&durable_steps::OperationStatus>) -> bool {
    matches!(
        value.map(|record| &record.state),
        Some(OperationState::Completed { .. })
    )
}

async fn revoke(client: &ConformanceClient) -> ConformanceResult<()> {
    let answer = revoke_answer(client).await?;
    if answer != json!({"kind":"machineRevoked"}) {
        return Err(format!("active-row revoke returned {answer}").into());
    }
    Ok(())
}
async fn revoke_answer(client: &ConformanceClient) -> ConformanceResult<Value> {
    let answer: Value = client
        .with_owner_access(
            client
                .client
                .post(format!("{}/admin/v1/machines/revoke", client.base_url)),
        )?
        .json(&json!({"kind":"revokeMachine","value":{"installation":SERVICE}}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(answer)
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
    ready: impl Fn(&RevocationObservation) -> bool,
) -> ConformanceResult<RevocationObservation> {
    let mut last = None;
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let value = control(client, RevocationControl::Observe).await?;
            if ready(&value) {
                return Ok(value);
            }
            last = Some(value);
        }
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => Err(format!("lifecycle readiness timed out; last owning state: {last:?}").into()),
    }
}
