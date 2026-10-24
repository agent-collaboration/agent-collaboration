//! A held registered renewal survives authenticated replacement of its connection.
#![cfg(feature = "conformance")]
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
    ConformanceResult, DispatchFaultObservation, RuntimeControl, RuntimeControlRequest,
    RuntimeFaultObservation, machine_assertion, owner_assertion,
};
use collaboration_central_core::ObjectAddress;
use durable_steps::Uuid;
use fleet_sync_protocol::*;
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;

const SERVICE: &str = "550e8400-e29b-41d4-a716-446655440751";
const CREDENTIAL: &str = "normal Move old holder";

fn session_address() -> ConformanceResult<ObjectAddress> {
    Ok(ObjectAddress::Connection(
        message_board::ServiceId::try_from(SERVICE.to_owned())?,
    ))
}
async fn control(
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
            address: session_address()?,
            command,
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
async fn receive(socket: &mut WebSocket) -> ConformanceResult<ServerFrame<BoardConsumerProtocol>> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or("inflight replacement socket ended before its data frame")??;
            let frame = match message {
                Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
                Message::Text(text) => serde_json::from_str(&text)?,
                other => {
                    return Err(format!(
                        "inflight replacement expected a data frame, received {other:?}"
                    )
                    .into());
                }
            };
            if !matches!(frame, ServerFrame::Notice { .. }) {
                return Ok(frame);
            }
        }
    })
    .await?
}
async fn replacement(
    base: &str,
    session_id: SessionId,
    database: DatabaseId,
) -> ConformanceResult<WebSocket> {
    let mut socket = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()?
        .get(format!("{base}/v1/session"))
        .bearer_auth(CREDENTIAL)
        .header("Cf-Access-Jwt-Assertion", machine_assertion(CREDENTIAL)?)
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                session_id,
                schema_support: SchemaSupport { max_version: 1 },
            })?
            .into(),
        ))
        .await?;
    match receive(&mut socket).await? {
        ServerFrame::Welcome {
            session_id: actual,
            databases,
        } if actual == session_id
            && databases
                == vec![DatabaseStatus::Ready {
                    database,
                    acked: fixture::position(1, 1)?,
                    lease: LeaseStatus {
                        remaining_ms: 13_500_000,
                        ..fixture::lease(database, 1, fixture::OLD_PEER)?
                    },
                }] =>
        {
            Ok(socket)
        }
        _ => Err("replacement Welcome lost the exact held session/schema/prefix/lease".into()),
    }
}
fn exact_held_renewal(held: &Value, request: &RenewRequest) -> ConformanceResult<SessionId> {
    let expected = serde_json::to_value(LeaseRequest::Renew(request.clone()))?;
    if held.get("kind") != Some(&json!("central.session-lease"))
        || held.get("source") != Some(&serde_json::to_value(session_address()?)?)
        || held.get("sourceFace") != Some(&json!("session"))
        || held.get("destination") != Some(&serde_json::to_value(ObjectAddress::Registry)?)
        || held.pointer("/request/session/peer") != Some(&json!(fixture::OLD_PEER))
        || held.pointer("/request/request") != Some(&expected)
    {
        return Err(
            "held dispatch is not this exact registered holder/session renewal request".into(),
        );
    }
    Ok(serde_json::from_value(
        held.pointer("/request/session/sessionId")
            .cloned()
            .ok_or("held renewal omitted its actual session identity")?,
    )?)
}

#[tokio::test]
async fn exact_held_renewal_resends_on_the_same_session_replacement_and_then_replays_exactly()
-> ConformanceResult<()> {
    // Reuse the unchanged normal enrollment/Fleet/Register/schema fixture; no Move is performed.
    let mut fixture = fixture::Fixture::open().await?;
    let mut new_socket = None;
    let proof=async {
        creation_scenarios::pin_registry_clock(fixture.base(),collaboration_central_core::RegistryTime(
            chrono::DateTime::from_timestamp_millis(fixture::time(900_000)?.unix_millis()).ok_or("inflight Registry time outside range")?
        )).await?;
        let request=RenewRequest {attempt:RequestId::new(Uuid::now_v7())?,database:fixture.database,epoch:Epoch::new(1)?};
        control(fixture.base(),RuntimeControl::HoldNextDispatch {destination:ObjectAddress::Registry}).await?;
        let original_frame=creation_scenarios::send(&mut fixture.old,OperationRequest::Lease(LeaseRequest::Renew(request.clone()))).await?;
        let DispatchFaultObservation::Held {destination,request:bytes}=control(fixture.base(),RuntimeControl::AwaitHeldDispatch).await?.dispatch else {return Err("registered renewal did not reach the real owned hold".into());};
        if destination!=ObjectAddress::Registry {return Err("inflight renewal hold targeted another object".into());}
        let held:Value=serde_json::from_slice(&bytes)?;
        let session_id=exact_held_renewal(&held,&request)?;
        println!("SESSION_INFLIGHT_STAGE exact_original_renewal_held passed");
        new_socket=Some(replacement(fixture.base(),session_id,fixture.database).await?);
        println!("SESSION_INFLIGHT_STAGE exact_same_session_replacement_Welcome passed");
        if !matches!(receive(&mut fixture.old).await?,ServerFrame::Closed {reason:CloseReason::ConnectionReplaced}) {
            return Err("held old connection did not receive exact typed ConnectionReplaced".into());
        }
        if !matches!(tokio::time::timeout(Duration::from_secs(5),fixture.old.next()).await?,Some(Ok(Message::Close {..}))|None) {
            return Err("held old connection was not physically closed after replacement".into());
        }
        println!("SESSION_INFLIGHT_STAGE same_session_Welcome_and_old_typed_physical_close passed");
        // Native may cancel the old call on physical close. Same-key retry must
        // settle safely whether that original call committed or was cancelled.
        control(fixture.base(),RuntimeControl::ResumeDispatch).await?;
        let socket=new_socket.as_mut().ok_or("replacement socket missing")?;
        let answer=creation_scenarios::operation_answer(socket,OperationRequest::Lease(LeaseRequest::Renew(request.clone()))).await?;
        let OperationAnswer::Lease(LeaseAnswer::Renew(RenewAnswer::Renewed {replayed,..}))=&answer else {return Err("replacement resend did not return its actual renewal result".into());};
        let replayed=*replayed;
        if !matches!(answer,OperationAnswer::Lease(LeaseAnswer::Renew(actual)) if actual==RenewAnswer::Renewed {
            recorded:RenewResult {epoch:Epoch::new(1)?,expires_at:fixture::time(15_300_000)?},
            current:LeaseStatus {expires_at:fixture::time(15_300_000)?,..fixture::lease(fixture.database,1,fixture::OLD_PEER)?},transfer_in_progress:None,replayed,
        }) {return Err("replacement resend did not recover the exact settled renewal receipt/current lease".into());}
        println!("SESSION_INFLIGHT_STAGE same_key_resend_settled passed replayed={replayed}");
        let repeated=creation_scenarios::operation_answer(socket,OperationRequest::Lease(LeaseRequest::Renew(request))).await?;
        if !matches!(repeated,OperationAnswer::Lease(LeaseAnswer::Renew(actual)) if actual==RenewAnswer::Renewed {
            recorded:RenewResult {epoch:Epoch::new(1)?,expires_at:fixture::time(15_300_000)?},
            current:LeaseStatus {expires_at:fixture::time(15_300_000)?,..fixture::lease(fixture.database,1,fixture::OLD_PEER)?},transfer_in_progress:None,replayed:true,
        }) {return Err("same renewal on the replacement did not replay its exact settled receipt/current lease".into());}
        // Reading on that same replacement also rejects a misrouted old answer's
        // original frame through the helper's strict frame correlation.
        if !matches!(creation_scenarios::operation_answer(socket,OperationRequest::Replication(ReplicationRequest::SnapshotPage {database:fixture.database,cursor:None})).await?,OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page))) if page.database==fixture.database && page.position==fixture::position(1,1)? && page.schema.version==1 && page.table=="move_records" && page.rows.is_empty() && page.next.is_none()) {
            return Err("replacement read changed the original registered schema/prefix/snapshot".into());
        }
        println!("SESSION_INFLIGHT_CASE exact_held_renewal_replaced_and_settled_receipt_replayed passed original_frame={original_frame:?}");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = control(fixture.base(), RuntimeControl::ResumeDispatch).await;
    if let Some(socket) = new_socket {
        let _ = socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
    }
    let closed = fixture.close().await;
    proof?;
    closed
}
