//! Registered sessions retain typed credential rejection and audience boundaries.
#![cfg(feature = "conformance")]
#![allow(
    clippy::panic_in_result_fn,
    reason = "Literal assertions qualify permanent conformance cases."
)]
#[path = "native_http/support.rs"]
mod configuration;
#[allow(dead_code)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;
#[allow(dead_code)]
#[path = "session_move/fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/owner_registry_scenarios.rs"]
mod owner_registry_scenarios;

use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use collaboration_central_conformance::{
    ConformanceResult, RuntimeControl, RuntimeControlRequest, RuntimeFaultObservation,
    machine_assertion, owner_assertion,
};
use collaboration_central_core::{AccessRefusal, ObjectAddress, RegistryTime};
use durable_steps::Uuid;
use fleet_sync_protocol::*;
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;

#[path = "session_credentials/transport_refusal.rs"]
mod transport_refusal;

const SERVICE: &str = "550e8400-e29b-41d4-a716-446655440751";
const CREDENTIAL: &str = "normal Move old holder";

async fn upgrade(
    http: &reqwest::Client,
    base: &str,
    credential: &str,
) -> ConformanceResult<reqwest_websocket::UpgradeResponse> {
    Ok(http
        .get(format!("{base}/v1/session"))
        .bearer_auth(credential)
        .header("Cf-Access-Jwt-Assertion", machine_assertion(credential)?)
        .upgrade()
        .send()
        .await?)
}
async fn rejected_upgrade(
    http: &reqwest::Client,
    base: &str,
    credential: &str,
    expected: AuthRefusal,
) -> ConformanceResult<()> {
    let response = upgrade(http, base, credential).await?;
    let status = if expected == AuthRefusal::WrongKind {
        reqwest::StatusCode::FORBIDDEN
    } else {
        reqwest::StatusCode::UNAUTHORIZED
    };
    if response.status() != status {
        return Err(format!(
            "credential upgrade did not refuse exact {expected:?}: {}",
            response.status()
        )
        .into());
    }
    let reasons = response
        .headers()
        .get_all("fleet-sync-auth-refusal")
        .iter()
        .collect::<Vec<_>>();
    if reasons.len() != 1
        || serde_json::from_slice::<AuthRefusal>(
            reasons.first().ok_or("missing refusal header")?.as_bytes(),
        )? != expected
    {
        return Err(
            format!("credential upgrade omitted exact single typed header {expected:?}").into(),
        );
    }
    if response.into_inner().json::<AuthRefusal>().await? != expected {
        return Err(format!("credential upgrade lost its typed {expected:?} refusal").into());
    }
    transport_refusal::rejected(
        base,
        credential,
        credential,
        expected,
        transport_refusal::HeaderMode::Original,
    )
    .await?;
    if expected == AuthRefusal::Expired {
        for mode in [
            transport_refusal::HeaderMode::Missing,
            transport_refusal::HeaderMode::Malformed,
            transport_refusal::HeaderMode::Repeated,
        ] {
            transport_refusal::rejected(base, credential, credential, AuthRefusal::Unknown, mode)
                .await?;
        }
    }
    Ok(())
}
async fn typed_physical_close(
    socket: &mut WebSocket,
    expected: AuthRefusal,
) -> ConformanceResult<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or("credential session ended before its typed close")??;
            let frame = match message {
                Message::Binary(bytes) => {
                    serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)?
                }
                Message::Text(text) => serde_json::from_str(&text)?,
                other => {
                    return Err(format!(
                        "credential session expected a typed close, received {other:?}"
                    )
                    .into());
                }
            };
            match frame {
                ServerFrame::Notice { .. } => {}
                ServerFrame::Closed {
                    reason: CloseReason::CredentialRejected(actual),
                } if actual == expected => break,
                _ => {
                    return Err(format!(
                        "registered session did not return exact CredentialRejected({expected:?})"
                    )
                    .into());
                }
            }
        }
        if !matches!(socket.next().await, Some(Ok(Message::Close { .. })) | None) {
            return Err("credential rejection was not followed by physical socket close".into());
        }
        Ok(())
    })
    .await?
}
fn expected_renewal(database: DatabaseId) -> ConformanceResult<RenewAnswer> {
    Ok(RenewAnswer::Renewed {
        recorded: RenewResult {
            epoch: Epoch::new(1)?,
            expires_at: fixture::time(14_400_000)?,
        },
        current: fixture::lease(database, 1, fixture::OLD_PEER)?,
        transfer_in_progress: None,
        replayed: false,
    })
}

async fn next_data(
    socket: &mut WebSocket,
) -> ConformanceResult<ServerFrame<BoardConsumerProtocol>> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or("replacement credential socket ended before data")??;
            let frame = match message {
                Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
                Message::Text(text) => serde_json::from_str(&text)?,
                other => {
                    return Err(
                        format!("replacement credential expected data, got {other:?}").into(),
                    );
                }
            };
            if !matches!(frame, ServerFrame::Notice { .. }) {
                return Ok(frame);
            }
        }
    })
    .await?
}
async fn connection_control(
    base: &str,
    command: RuntimeControl,
) -> ConformanceResult<RuntimeFaultObservation> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()?
        .post(format!("{base}/admin/v1/conformance/runtime"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&RuntimeControlRequest {
            address: ObjectAddress::Connection(message_board::ServiceId::try_from(
                SERVICE.to_owned(),
            )?),
            command,
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
async fn replace_registered_socket_before_revocation(
    fixture: &mut fixture::Fixture,
) -> ConformanceResult<()> {
    // Capture the real current session ID from an exact, normally completed
    // renewal, using the existing dispatch control rather than private storage.
    let request = RenewRequest {
        attempt: RequestId::new(Uuid::now_v7())?,
        database: fixture.database,
        epoch: Epoch::new(1)?,
    };
    connection_control(
        fixture.base(),
        RuntimeControl::HoldNextDispatch {
            destination: ObjectAddress::Registry,
        },
    )
    .await?;
    let proof=async {
        let frame=creation_scenarios::send(&mut fixture.old,OperationRequest::Lease(LeaseRequest::Renew(request.clone()))).await?;
        let observation=connection_control(fixture.base(),RuntimeControl::AwaitHeldDispatch).await?;
        let collaboration_central_conformance::DispatchFaultObservation::Held {destination,request:bytes}=observation.dispatch else {return Err("credential replacement did not hold its real renewal".into());};
        let held:Value=serde_json::from_slice(&bytes)?;
        if destination!=ObjectAddress::Registry || held.get("kind")!=Some(&json!("central.session-lease"))
            || held.pointer("/request/session/peer")!=Some(&json!(fixture::OLD_PEER))
            || held.pointer("/request/request")!=Some(&serde_json::to_value(LeaseRequest::Renew(request))?) {
            return Err("credential replacement captured another holder renewal".into());
        }
        let session_id:SessionId=serde_json::from_value(held.pointer("/request/session/sessionId").cloned().ok_or("credential renewal omitted current session ID")?)?;
        connection_control(fixture.base(),RuntimeControl::ResumeDispatch).await?;
        if !matches!(next_data(&mut fixture.old).await?,ServerFrame::Answer {frame:actual,body:OperationAnswer::Lease(LeaseAnswer::Renew(answer))} if actual==frame && answer==expected_renewal(fixture.database)?) {return Err("captured current-session renewal did not complete exactly".into());}
        let mut replacement=upgrade(&fixture.http,fixture.base(),CREDENTIAL).await?.into_websocket().await?;
        replacement.send(Message::Binary(serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {session_id,schema_support:SchemaSupport {max_version:1}})?.into())).await?;
        if !matches!(next_data(&mut replacement).await?,ServerFrame::Welcome {session_id:actual,databases} if actual==session_id && databases==vec![DatabaseStatus::Ready {database:fixture.database,acked:fixture::position(1,1)?,lease:fixture::lease(fixture.database,1,fixture::OLD_PEER)?}]) {return Err("pre-revocation replacement lost the exact registered Welcome".into());}
        if !matches!(next_data(&mut fixture.old).await?,ServerFrame::Closed {reason:CloseReason::ConnectionReplaced}) {return Err("pre-revocation old socket lost typed replacement close".into());}
        if !matches!(tokio::time::timeout(Duration::from_secs(5),fixture.old.next()).await?,Some(Ok(Message::Close {..}))|None) {return Err("pre-revocation old socket did not physically close".into());}
        let old=std::mem::replace(&mut fixture.old,replacement);
        let _=old.close(reqwest_websocket::CloseCode::Normal,None).await;
        if fixture.renew_old().await?!=expected_renewal(fixture.database)? {return Err("pre-revocation replacement did not become the usable registered holder".into());}
        println!("SESSION_CREDENTIAL_STAGE registered_replacement_is_current_before_revocation passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = connection_control(fixture.base(), RuntimeControl::ResumeDispatch).await;
    proof
}

async fn reconnect_unrelated_installation_after_revocation(
    fixture: &mut fixture::Fixture,
) -> ConformanceResult<()> {
    let response = upgrade(&fixture.http, fixture.base(), "normal Move target").await?;
    if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS {
        let status = response.status();
        let body = response.into_inner().json::<Value>().await?;
        return Err(format!(
            "revocation refused the unrelated installation's fresh upgrade: {status}: {body}"
        )
        .into());
    }
    let mut socket = response.into_websocket().await?;
    let session_id = SessionId::new(Uuid::now_v7())?;
    let proof = async {
        socket
            .send(Message::Binary(
                serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                    session_id,
                    schema_support: SchemaSupport { max_version: 10 },
                })?
                .into(),
            ))
            .await?;
        if !matches!(next_data(&mut socket).await?,ServerFrame::Welcome {session_id:actual,databases} if actual==session_id && databases.is_empty())
        {
            return Err(
                "unrelated reconnect did not complete its exact fresh Hello/empty Welcome".into(),
            );
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    if let Err(error) = proof {
        let _ = socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        return Err(error);
    }
    // The fixture owns the replacement for shutdown. The original target
    // socket has been replaced by this real Hello and is no longer the readback.
    let previous = std::mem::replace(&mut fixture.target, socket);
    let _ = previous
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    fixture.copy_from_target_unchanged().await?;
    println!(
        "SESSION_REVOCATION_CASE unrelated_fresh_upgrade_Hello_empty_Welcome_and_exact_Copy_read passed"
    );
    Ok(())
}

#[tokio::test]
async fn revoked_registered_machine_gets_typed_physical_close_and_cannot_upgrade_again()
-> ConformanceResult<()> {
    let mut fixture = fixture::Fixture::open().await?;
    let proof = async {
        if fixture.renew_old().await? != expected_renewal(fixture.database)? {
            return Err("revocation fixture was not a live registered holder".into());
        }
        replace_registered_socket_before_revocation(&mut fixture).await?;
        let response = fixture
            .http
            .post(format!("{}/admin/v1/machines/revoke", fixture.base()))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&json!({"kind":"revokeMachine","value":{"installation":SERVICE}}))
            .send()
            .await?;
        if response.status() != reqwest::StatusCode::OK
            || response.json::<Value>().await? != json!({"kind":"machineRevoked"})
        {
            return Err(
                "registered revocation did not return its actual committed MachineRevoked decision"
                    .into(),
            );
        }
        println!("SESSION_CREDENTIAL_STAGE actual_registered_machine_revoked passed");
        typed_physical_close(&mut fixture.old, AuthRefusal::Revoked).await?;
        println!(
            "SESSION_CREDENTIAL_STAGE registered_revoked_socket_typed_and_physically_closed passed"
        );
        rejected_upgrade(
            &fixture.http,
            fixture.base(),
            CREDENTIAL,
            AuthRefusal::Revoked,
        )
        .await?;
        fixture.copy_from_target_unchanged().await?;
        reconnect_unrelated_installation_after_revocation(&mut fixture).await?;
        println!(
            "SESSION_CREDENTIAL_CASE revoked_registered_close_upgrade_and_retained_Copy passed"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = fixture.close().await;
    proof?;
    closed
}

#[tokio::test]
async fn expired_machine_credential_closes_on_frame_and_refuses_a_fresh_access_verified_upgrade()
-> ConformanceResult<()> {
    let mut fixture = fixture::Fixture::open().await?;
    let proof=async {
        if fixture.renew_old().await?!=expected_renewal(fixture.database)? {return Err("expiry fixture was not a live registered holder".into());}
        // The reused normal enrollment verifies its requested2033 expiry. Only
        // the owning Registry and old Connection clocks advance beyond it.
        let now=RegistryTime("2033-05-18T03:33:20.001Z".parse()?);
        for address in [ObjectAddress::Registry,ObjectAddress::Connection(message_board::ServiceId::try_from(SERVICE.to_owned())?)] {
            let observed:RuntimeFaultObservation=fixture.http.post(format!("{}/admin/v1/conformance/runtime",fixture.base()))
                .header("Cf-Access-Jwt-Assertion",owner_assertion()?)
                .json(&RuntimeControlRequest {address,command:RuntimeControl::SetTime {now}})
                .send().await?.error_for_status()?.json().await?;
            if !matches!(observed.clock,collaboration_central_conformance::ClockObservation::Pinned {now:actual} if actual==now) {return Err("credential expiry clock did not reach its exact owning object".into());}
        }
        println!("SESSION_CREDENTIAL_STAGE actual_enrollment_expiry_crossed_on_Registry_and_Connection passed");
        creation_scenarios::send(&mut fixture.old,OperationRequest::Lease(LeaseRequest::Renew(RenewRequest {attempt:RequestId::new(Uuid::now_v7())?,database:fixture.database,epoch:Epoch::new(1)?}))).await?;
        typed_physical_close(&mut fixture.old,AuthRefusal::Expired).await?;
        println!("SESSION_CREDENTIAL_STAGE expired_binding_refused_on_frame_and_physically_closed passed");
        // Access JWT verification keeps its real wall clock. The fresh signed
        // assertion is valid; the stored machine credential is what has expired.
        rejected_upgrade(&fixture.http,fixture.base(),CREDENTIAL,AuthRefusal::Expired).await?;
        fixture.copy_from_target_unchanged().await?;
        println!("SESSION_CREDENTIAL_CASE expired_machine_frame_upgrade_and_retained_Copy passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = fixture.close().await;
    proof?;
    closed
}

#[tokio::test]
async fn unregistered_token_owner_upgrade_and_machine_admin_are_refused_without_poisoning_the_live_holder()
-> ConformanceResult<()> {
    let mut fixture = fixture::Fixture::open().await?;
    let proof = async {
        if fixture.renew_old().await? != expected_renewal(fixture.database)? {
            return Err("audience fixture was not a live registered holder".into());
        }
        println!("SESSION_CREDENTIAL_STAGE actual_registered_holder_live_before_refusals passed");
        // This is the old un-enrolled machine-or-agent token assertion, not a
        // claim to exercise a real Router agent credential producer.
        rejected_upgrade(
            &fixture.http,
            fixture.base(),
            "unknown-machine-or-agent-credential",
            AuthRefusal::Unknown,
        )
        .await?;
        let owner = fixture
            .http
            .get(format!("{}/v1/session", fixture.base()))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .upgrade()
            .send()
            .await?;
        let status=owner.status();let body=owner.into_inner().json::<Value>().await?;
        if status!=reqwest::StatusCode::UNAUTHORIZED || body!=json!({"kind":"wrongAudience"}) || serde_json::from_value::<AccessRefusal>(body.clone())?!=AccessRefusal::WrongAudience {
            return Err(format!("owner Access assertion acquired a machine session or lost exact WrongAudience: {status}: {body}").into());
        }
        let machine = fixture
            .http
            .post(format!(
                "{}/admin/v1/projects/{}/takeover/preview",
                fixture.base(),
                fixture.database
            ))
            .bearer_auth(CREDENTIAL)
            .header("Cf-Access-Jwt-Assertion", machine_assertion(CREDENTIAL)?)
            .json(&Value::Null)
            .send()
            .await?;
        let status=machine.status();let body=machine.json::<Value>().await?;
        if status!=reqwest::StatusCode::UNAUTHORIZED || body!=json!({"kind":"wrongAudience"}) || serde_json::from_value::<AccessRefusal>(body.clone())?!=AccessRefusal::WrongAudience {
            return Err(format!("valid machine credential acquired owner admin or lost exact WrongAudience: {status}: {body}").into());
        }
        if fixture.renew_old().await? != expected_renewal(fixture.database)? {
            return Err(
                "refused identities poisoned the actual registered holder's session".into(),
            );
        }
        fixture
            .copy_unchanged(1, fixture::position(1, 1)?, false)
            .await?;
        println!(
            "SESSION_CREDENTIAL_CASE unknown_owner_machine_audiences_and_live_holder_control passed"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = fixture.close().await;
    proof?;
    closed
}

#[tokio::test]
async fn credential_with_another_access_service_binding_gets_wrong_kind_403()
-> ConformanceResult<()> {
    let fixture = fixture::Fixture::open().await?;
    let proof = async {
        let response = fixture
            .http
            .get(format!("{}/v1/session", fixture.base()))
            .bearer_auth(CREDENTIAL)
            .header(
                "Cf-Access-Jwt-Assertion",
                machine_assertion("normal Move target")?,
            )
            .upgrade()
            .send()
            .await?;
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        assert_eq!(
            response
                .headers()
                .get("fleet-sync-auth-refusal")
                .ok_or("missing WrongKind header")?
                .to_str()?,
            "\"wrongKind\""
        );
        assert_eq!(
            response.into_inner().json::<AuthRefusal>().await?,
            AuthRefusal::WrongKind
        );
        transport_refusal::rejected(
            fixture.base(),
            CREDENTIAL,
            "normal Move target",
            AuthRefusal::WrongKind,
            transport_refusal::HeaderMode::Original,
        )
        .await
    }
    .await;
    fixture.close().await?;
    proof
}
