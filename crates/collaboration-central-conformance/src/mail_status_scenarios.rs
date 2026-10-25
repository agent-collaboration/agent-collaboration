//! Normal session status reads use the connection's own stream and cursor.

use crate::{ConformanceClient, ConformanceResult, enroll_admitted_peer};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::Message;
use serde_json::{Value, json};
use std::time::Duration;

pub async fn run_empty_status_stream_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let mut admitted = enroll_admitted_peer(
        client,
        "550e8400-e29b-41d4-a716-4466554400d1",
        "019f0000-0000-7000-8000-000000001004",
        "status-stream-reader",
        "019f0000-0000-7000-8000-000000000d11",
    )
    .await?;
    let proof = async {
        for after in [0, 1, 0] {
            let frame = uuid::Uuid::now_v7();
            let request = json!({"kind":"request","value":{
                "frame":frame,"body":{"kind":"delivery","value":{"kind":"statusFetch","value":{
                    "call":{"operation":uuid::Uuid::now_v7(),"step":"status_fetch"},"after":after,"limit":10
                }}}
            }});
            admitted.socket.send(Message::Text(request.to_string())).await?;
            let received = tokio::time::timeout(Duration::from_secs(5), admitted.socket.next()).await?
                .ok_or("status session ended before its answer")??;
            let answer: Value = match received {
                Message::Text(text) => serde_json::from_str(&text)?,
                Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
                other => return Err(format!("expected a status answer, got {other:?}").into()),
            };
            let body = if after == 0 {
                json!({"kind":"delivery","value":{"kind":"statusFetch","value":{"events":[],"through":0}}})
            } else {
                json!({"kind":"invalidShape","value":{"field":"after"}})
            };
            if answer != json!({"kind":"answer","value":{"frame":frame,"body":body}}) {
                return Err(format!("wrong correlated status cursor answer: {answer}").into());
            }
        }
        println!("MAIL_STATUS_CASE empty_stream_future_cursor_refusal_and_next_read_share_normal_session passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let closed = admitted
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}
