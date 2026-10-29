//! Actual WebSocket peers exercise fixture admission, overlap and owner completion.
use super::{
    ConcurrentUpstreamConfig, MockConcurrentWebSocketUpstream, SMOKE_EXPECTED_TEXT,
    SMOKE_TARGET_MODEL,
};
use crate::FixtureResult;
use std::time::Duration;
use tungstenite::{Message, client::IntoClientRequest as _, stream::MaybeTlsStream};

#[test]
fn actual_peer_completion_wakes_accept_and_preserves_all_overlap_receipts() -> FixtureResult {
    // Duration is outside this owner's claim: this case proves real protocol
    // rendezvous/completion. The separately named physical soak stays real.
    let upstream = MockConcurrentWebSocketUpstream::start(
        ConcurrentUpstreamConfig {
            expected_sessions: 3,
            expected_upstream_sessions: 3,
            hold_duration: Duration::ZERO,
            heartbeat_interval: Duration::ZERO,
        },
        None,
    )?;
    let address = upstream.address().to_owned();
    let peers = std::thread::scope(|scope| -> FixtureResult {
        let mut peers = Vec::new();
        for index in 0..3 {
            let address = &address;
            peers.push(scope.spawn(move || run_peer(address, index)));
        }
        for peer in peers {
            peer.join().map_err(|_| "fixture peer panicked")??;
        }
        Ok(())
    });
    peers?;
    let observed = upstream.join()?;
    if observed.completed_sessions != 3
        || observed.final_active_sessions != 0
        || observed.active_high_water != 3
        || observed.target_model_session_count != 3
        || observed.upstream_client_sessions.len() != 3
        || !observed.multi_step_interleave_completed
        || observed.multi_step_followup_frame_count != 1
        || observed.session_frame_counts.iter().sum::<usize>() != 4
        || observed.in_overlap_session_event_counts.len() != 3
        || observed
            .in_overlap_session_event_counts
            .iter()
            .any(|count| *count < 3)
    {
        return Err(
            format!("real peers lost their exact completion/overlap facts: {observed:?}").into(),
        );
    }
    Ok(())
}

fn run_peer(address: &str, index: usize) -> FixtureResult {
    let mut request = format!("ws://{address}/v1/responses").into_client_request()?;
    request
        .headers_mut()
        .insert("authorization", "Bearer fixture-peer".parse()?);
    let (mut socket, _) = tungstenite::connect(request)?;
    let MaybeTlsStream::Plain(stream) = socket.get_mut() else {
        return Err("fixture peer did not use its actual plain loopback socket".into());
    };
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.send(Message::text(serde_json::json!({
        "type":"response.create", "model":SMOKE_TARGET_MODEL, "stream":true,
        "input":[{"role":"user","content":[{"type":"input_text","text":format!("fixture codex-router-client-{index}")}]}]
    }).to_string()))?;
    let mut tool_call = None;
    let mut completed = 0;
    let mut saw_text = false;
    loop {
        match socket.read()? {
            Message::Text(text) => {
                saw_text |= text.contains(SMOKE_EXPECTED_TEXT);
                let event: serde_json::Value = serde_json::from_str(&text)?;
                if event.get("type").and_then(serde_json::Value::as_str)
                    == Some("response.output_item.done")
                    && event
                        .pointer("/item/type")
                        .and_then(serde_json::Value::as_str)
                        == Some("function_call")
                {
                    tool_call = Some(
                        event
                            .pointer("/item/call_id")
                            .and_then(serde_json::Value::as_str)
                            .ok_or("fixture tool call omitted its ID")?
                            .to_owned(),
                    );
                }
                if event.get("type").and_then(serde_json::Value::as_str)
                    == Some("response.completed")
                {
                    completed += 1;
                    if let Some(call_id) = tool_call.take() {
                        socket.send(Message::text(serde_json::json!({"type":"response.create", "model":SMOKE_TARGET_MODEL, "stream":true, "input":[{"type":"function_call_output","call_id":call_id,"output":"fixture tool output"}]}).to_string()))?;
                    }
                }
            }
            Message::Close(_) => break,
            Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }
    if completed == 0 || !saw_text {
        return Err("fixture peer closed without the real completed response text".into());
    }
    Ok(())
}
