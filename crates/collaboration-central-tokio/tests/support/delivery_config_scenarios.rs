//! Every enrollment answer exposes the deployment's configured relay identity.
use collaboration_central_conformance::{ConformanceResult, access_client_id, owner_assertion};
use collaboration_central_core::{
    RevocationControl, RevocationControlInput, RevocationObservation,
};
use durable_steps::OperationState;
use fleet_sync_protocol::PeerId;
use serde_json::{Value, json};
use std::time::Duration;

pub(super) async fn run(base_url: &str, expected_relay: PeerId) -> ConformanceResult<()> {
    run_case(
        base_url,
        expected_relay,
        "550e8400-e29b-41d4-a716-446655445240",
        "019f0000-0000-7000-8000-000000005249",
        "delivery-config",
    )
    .await
}

async fn run_case(
    base_url: &str,
    expected_relay: PeerId,
    installation: &str,
    peer: &str,
    client_token: &str,
) -> ConformanceResult<()> {
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let marker = "DELIVERY_CONFIG_CASE";
    let (initial_hash, updated_hash) = ("a".repeat(64), "b".repeat(64));
    let mut body = json!({"kind":"enrollMachine","value":{
        "installation":installation,"peer":peer,"label":"configured relay enrollment",
        "access_client_id":access_client_id(client_token)?,
        "credential_hash":initial_hash,"expires_at":"2033-05-18T03:33:20Z"
    }});
    for stage in ["first", "exact-replay", "active-update"] {
        if stage == "active-update" {
            *body
                .pointer_mut("/value/label")
                .ok_or("relay fixture label missing")? = json!("configured relay active update");
            *body
                .pointer_mut("/value/credential_hash")
                .ok_or("relay fixture credential missing")? = json!(updated_hash);
        }
        let answer = owner_post(&http, base_url, "enroll", &body).await?;
        enrollment(&answer, peer, expected_relay)?;
        println!("{marker} {stage} configured_relay passed");
    }
    let revoked = owner_post(
        &http,
        base_url,
        "revoke",
        &json!({"kind":"revokeMachine","value":{"installation":installation}}),
    )
    .await?;
    if revoked != json!({"kind":"machineRevoked"}) {
        return Err("relay configuration fixture did not complete real revocation".into());
    }
    await_lifecycle(&http, base_url, installation, Lifecycle::Revocation).await?;
    let readmitted = owner_post(&http, base_url, "enroll", &body).await?;
    if readmitted == json!({"kind":"enrollmentInProgress"}) {
        println!("{marker} readmission acknowledged pending");
    } else {
        enrollment(&readmitted, peer, expected_relay)?;
    }
    await_lifecycle(&http, base_url, installation, Lifecycle::Readmission).await?;
    for stage in ["readmission-completed", "readmission-replay"] {
        let answer = owner_post(&http, base_url, "enroll", &body).await?;
        enrollment(&answer, peer, expected_relay)?;
        println!("{marker} {stage} configured_relay passed");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum Lifecycle {
    Revocation,
    Readmission,
}
async fn await_lifecycle(
    http: &reqwest::Client,
    base_url: &str,
    installation: &str,
    lifecycle: Lifecycle,
) -> ConformanceResult<()> {
    // MachineRevoked records admission. Reuse the owning journal observation
    // to establish completion; a response or a delay cannot establish it.
    let mut last = None;
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let observed: RevocationObservation = http
                .post(format!("{base_url}/admin/v1/conformance/revocation"))
                .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
                .json(&RevocationControlInput {
                    service: installation.to_owned().try_into()?,
                    control: RevocationControl::Observe,
                })
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            if let Some(row) = observed.enrollment.as_ref() {
                let (operation, expected, revoked) = match lifecycle {
                    Lifecycle::Revocation => (
                        observed.operation.as_ref(),
                        row.last_revocation_operation,
                        true,
                    ),
                    Lifecycle::Readmission => (
                        observed.readmission_operation.as_ref(),
                        row.last_enrollment_operation,
                        false,
                    ),
                };
                if let Some(operation) = operation {
                    match &operation.state {
                        OperationState::Completed { .. }
                            if row.revoked == revoked && expected == Some(operation.id) =>
                        {
                            return Ok(());
                        }
                        OperationState::Failed { reason } => {
                            return Err(
                                format!("relay fixture lifecycle failed: {reason:?}").into()
                            );
                        }
                        _ => {}
                    }
                }
            }
            last = Some(observed);
        }
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => Err(format!(
            "relay fixture {lifecycle:?} readiness timed out; last owning state: {last:?}"
        )
        .into()),
    }
}

fn enrollment(answer: &Value, peer: &str, relay: PeerId) -> ConformanceResult<()> {
    let expected = json!({"kind":"machineEnrolled","value":{
        "peer":peer,"expiresAt":"2033-05-18T03:33:20Z",
        "authoritativeReceiver":"019f0000-0000-7000-8000-000000000010",
        "relayPeer":relay
    }});
    if answer != &expected {
        return Err(
            format!("enrollment did not return the exact configured identities: {answer}").into(),
        );
    }
    if relay.to_string() == "019f0000-0000-7000-8000-000000000010" {
        return Err("relay proof must configure an identity different from its receiver".into());
    }
    Ok(())
}
async fn owner_post(
    http: &reqwest::Client,
    base_url: &str,
    operation: &str,
    body: &Value,
) -> ConformanceResult<Value> {
    Ok(http
        .post(format!("{base_url}/admin/v1/machines/{operation}"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
