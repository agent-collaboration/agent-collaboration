//! Test-only stdin queries observe committed local cancellation.
use super::shutdown_signals::ShutdownSignals;
use codex_router_host::CollaborationRuntime;
use fleet_sync_protocol::ItemId;
use serde::Deserialize;
use std::io;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum MailQuery {
    LocalCancelDelivery { cancel: ItemId },
}

pub(super) async fn serve_until_shutdown(
    runtime: &mut CollaborationRuntime,
    shutdown_signals: &mut ShutdownSignals,
) -> io::Result<()> {
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let mut input_open = true;
    let mut output = tokio::io::stdout();
    loop {
        let line = tokio::select! {
            result = shutdown_signals.wait_for_signal() => return result,
            error = runtime.listener_failure() => return Err(io::Error::other(error)),
            line = input.next_line(), if input_open => line?,
        };
        let Some(line) = line else {
            input_open = false;
            continue;
        };
        let query: MailQuery = serde_json::from_str(&line).map_err(io::Error::other)?;
        let reply = match query {
            MailQuery::LocalCancelDelivery { cancel } => {
                let recorded = runtime
                    .runtime_test_has_local_cancel_delivery(cancel)
                    .await?;
                serde_json::json!({"kind": "localCancelDelivery", "cancel": cancel, "recorded": recorded})
            }
        };
        let mut reply = serde_json::to_vec(&reply).map_err(io::Error::other)?;
        reply.push(b'\n');
        output.write_all(&reply).await?;
        output.flush().await?;
    }
}
