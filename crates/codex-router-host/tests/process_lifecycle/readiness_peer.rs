//! The same native protocol responses, released after actual request receipts.
use codex_router_test_support::FixtureResult;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{future::Future, path::Path, pin::Pin, time::Duration};
use tokio::sync::oneshot;
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

pub(super) struct ReadinessPeer {
    pub native_requested: oneshot::Receiver<()>,
    pub remote_requested: oneshot::Receiver<()>,
    pub release_native: oneshot::Sender<()>,
    pub release_remote: oneshot::Sender<()>,
    pub task: tokio::task::JoinHandle<FixtureResult>,
}

pub(super) fn start(socket: &Path) -> std::io::Result<ReadinessPeer> {
    let listener = tokio::net::UnixListener::bind(socket)?;
    let (native_requested, native_request) = oneshot::channel();
    let (remote_requested, remote_request) = oneshot::channel();
    let (release_native, native_release) = oneshot::channel();
    let (release_remote, remote_release) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut socket = tokio_tungstenite::accept_async(stream).await?;
        let initialize = read_request(&mut socket, "initialize").await?;
        native_requested
            .send(())
            .map_err(|_| "native request observer closed")?;
        native_release.await?;
        socket
            .send(Message::text(
                json!({
                    "id":initialize.get("id"),"result":{"userAgent":"codex-cli/1.2.3"}
                })
                .to_string(),
            ))
            .await?;
        read_request(&mut socket, "initialized").await?;
        let status = read_request(&mut socket, "remoteControl/status/read").await?;
        socket
            .send(Message::text(
                json!({
                    "id":status.get("id"),"result":{"status":"connecting",
                        "serverName":"fixture","environmentId":"fixture"}
                })
                .to_string(),
            ))
            .await?;
        remote_requested
            .send(())
            .map_err(|_| "Remote Control observer closed")?;
        remote_release.await?;
        socket
            .send(Message::text(
                json!({
                    "method":"remoteControl/status/changed","params":{"status":"connected",
                        "serverName":"fixture","environmentId":"fixture"}
                })
                .to_string(),
            ))
            .await?;
        Ok(())
    });
    Ok(ReadinessPeer {
        native_requested: native_request,
        remote_requested: remote_request,
        release_native,
        release_remote,
        task,
    })
}

pub(super) async fn await_stage<Observation: Future>(
    mut observing: Pin<&mut Observation>,
    stage: oneshot::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>>
where
    Observation::Output: std::fmt::Debug,
{
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            result = &mut observing => Err(format!("readiness settled before actual phase: {result:?}").into()),
            received = stage => received.map_err(Into::<Box<dyn std::error::Error>>::into),
        }
    })
    .await?
}

async fn read_request(
    socket: &mut WebSocketStream<tokio::net::UnixStream>,
    expected: &str,
) -> FixtureResult<Value> {
    let frame = socket
        .next()
        .await
        .ok_or("native readiness connection closed")??;
    let request: Value = serde_json::from_str(frame.to_text()?)?;
    if request.get("method").and_then(Value::as_str) != Some(expected) {
        return Err(format!("expected native readiness {expected}: {request}").into());
    }
    Ok(request)
}
