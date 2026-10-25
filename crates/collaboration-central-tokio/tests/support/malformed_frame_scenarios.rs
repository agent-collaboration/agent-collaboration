//! Raw malformed frames preserve correlation, copy state and healthy sockets.

use super::{copy_read_scenarios as reads, copy_wait_scenarios::CopyClient};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer,
};
use fleet_sync_protocol::{DatabaseId, FrameId, ReplicationRequest};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
enum MalformedCase {
    CorrelatedBody,
    UndecodableJson,
    MissingRequestId,
    InvalidRequestId,
}

pub async fn run(base: &str) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    for case in [
        MalformedCase::CorrelatedBody,
        MalformedCase::UndecodableJson,
        MalformedCase::MissingRequestId,
        MalformedCase::InvalidRequestId,
    ] {
        run_case(&client, base, case).await?;
        println!("MALFORMED_CASE {case:?} passed");
    }
    Ok(())
}

async fn run_case(
    client: &ConformanceClient,
    base: &str,
    case: MalformedCase,
) -> ConformanceResult<()> {
    let session = durable_steps::Uuid::now_v7().to_string();
    let mut malformed = enroll_admitted_peer(
        client,
        "550e8400-e29b-41d4-a716-4466554400e5",
        "019f0000-0000-7000-8000-000000001027",
        "malformed-frame-carryover",
        &session,
    )
    .await?;
    let mut healthy = enroll_admitted_peer(
        client,
        "550e8400-e29b-41d4-a716-4466554400e6",
        "019f0000-0000-7000-8000-000000001028",
        "malformed-frame-healthy",
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await?;
    let proof = async {
        let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
        CopyClient::new(base)?.create(database).await?;
        // A real initialized shared copy supplies the same before/after read,
        // including its literal empty-page identity, position and schema.
        reads::page(&mut malformed.socket, database).await?;
        let baseline = reads::request(
            &mut malformed.socket,
            ReplicationRequest::CopyStatus { database },
        )
        .await?;
        let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
        let raw = match case {
            MalformedCase::CorrelatedBody => serde_json::to_vec(&json!({
                "kind":"request", "value":{
                    "frame":frame, "body":{"kind":"unknownOperation"}
                }
            }))?,
            MalformedCase::UndecodableJson => b"{".to_vec(),
            MalformedCase::MissingRequestId => {
                br#"{"kind":"request","value":{"body":{}}}"#.to_vec()
            }
            MalformedCase::InvalidRequestId => {
                br#"{"kind":"request","value":{"frame":"wrong","body":{}}}"#.to_vec()
            }
        };
        malformed.socket.send(Message::Binary(raw.into())).await?;
        let expected = match case {
            MalformedCase::CorrelatedBody => json!({"kind":"answer","value":{
                "frame":frame,"body":{"kind":"invalidShape","value":{"field":"body"}}
            }}),
            _ => json!({"kind":"closed","value":{"reason":{"kind":"malformedFrame"}}}),
        };
        let actual = receive(&mut malformed.socket).await.map_err(|error| {
            format!("malformed {case:?} did not get its typed refusal: {error}")
        })?;
        if actual != expected {
            return Err(format!("malformed {case:?} answer: {actual}; expected {expected}").into());
        }
        match case {
            MalformedCase::CorrelatedBody => {
                // A duplicate refusal or an uncorrelated reply is the next
                // frame and fails this exact fresh request's correlation.
                if reads::request(
                    &mut malformed.socket,
                    ReplicationRequest::CopyStatus { database },
                )
                .await?
                    != baseline
                {
                    return Err("correlated refusal changed the initialized copy".into());
                }
            }
            _ => {
                if !matches!(
                    tokio::time::timeout(Duration::from_secs(5), malformed.socket.next()).await?,
                    Some(Ok(Message::Close { .. })) | None
                ) {
                    return Err(
                        "typed malformed refusal did not physically close the socket".into(),
                    );
                }
            }
        }
        // The independently admitted socket must remain usable after either
        // outcome and read the unchanged committed copy, without reconnecting.
        if reads::request(
            &mut healthy.socket,
            ReplicationRequest::CopyStatus { database },
        )
        .await?
            != baseline
        {
            return Err("malformed frame changed the copy or the other healthy session".into());
        }
        reads::page(&mut healthy.socket, database).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let _ = malformed
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let closed = healthy
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
        .ok_or("malformed socket ended before its typed refusal")??;
    match message {
        Message::Text(text) => Ok(serde_json::from_str(&text)?),
        Message::Binary(bytes) => Ok(serde_json::from_slice(&bytes)?),
        other => Err(format!("malformed frame received no typed data refusal: {other:?}").into()),
    }
}
