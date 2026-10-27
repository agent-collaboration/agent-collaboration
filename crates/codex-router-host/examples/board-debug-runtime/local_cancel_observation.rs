//! Test-only stdin queries observe committed Local cancellation through its owner.
use codex_router_host::CollaborationRuntime;
use fleet_sync_protocol::ItemId;
use serde::Deserialize;
use std::io;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalCancelQuery {
    project: message_board::ProjectId,
    cancel: ItemId,
}

pub async fn serve_until_shutdown(runtime: &mut CollaborationRuntime) -> io::Result<()> {
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let mut input_open = true;
    let mut output = tokio::io::stdout();
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        let line = tokio::select! {
            result = tokio::signal::ctrl_c() => return result,
            _ = terminate.recv() => return Ok(()),
            error = runtime.listener_failure() => return Err(io::Error::other(error)),
            line = input.next_line(), if input_open => line?,
        };
        let Some(line) = line else {
            input_open = false;
            continue;
        };
        let query: LocalCancelQuery = serde_json::from_str(&line).map_err(io::Error::other)?;
        runtime
            .runtime_test_replicate_project(&query.project)
            .await?;
        let recorded = runtime
            .runtime_test_has_local_cancel_delivery(query.cancel)
            .await?;
        let mut reply = serde_json::to_vec(&serde_json::json!({
            "kind": "localCancelDelivery", "cancel": query.cancel, "recorded": recorded,
        }))
        .map_err(io::Error::other)?;
        reply.push(b'\n');
        output.write_all(&reply).await?;
        output.flush().await?;
    }
}
