//! A native socket stand-in observes the real sender's stable push correlation.
use super::*;

pub(super) struct WakeAcceptanceFixture {
    pub native: tokio::task::JoinHandle<TestResult<()>>,
    pub correlation: tokio::sync::oneshot::Receiver<String>,
}

pub(super) fn accepting_native_call(root: &Path) -> TestResult<WakeAcceptanceFixture> {
    let listener = tokio::net::UnixListener::bind(root.join("native.sock"))?;
    let (correlation_sender, correlation_receiver) = tokio::sync::oneshot::channel();
    let native = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut socket = tokio_tungstenite::accept_async(stream).await?;
        let init: Value =
            serde_json::from_str(socket.next().await.ok_or("missing init")??.to_text()?)?;
        socket
            .send(Message::Text(
                json!({"id":init["id"],"result":{}}).to_string().into(),
            ))
            .await?;
        let _initialized = socket.next().await.ok_or("missing initialized")??;
        let read: Value = serde_json::from_str(
            socket
                .next()
                .await
                .ok_or("missing native read")??
                .to_text()?,
        )?;
        if read.get("method") != Some(&json!("thread/read")) {
            return Err("wake delivery skipped the native residency read".into());
        }
        socket
            .send(Message::Text(
                json!({"id":read["id"],"result":{"thread":{"id":"fixture-thread","status":{"type":"idle"}}}})
                    .to_string()
                    .into(),
            ))
            .await?;
        let start: Value = serde_json::from_str(
            socket
                .next()
                .await
                .ok_or("missing native start")??
                .to_text()?,
        )?;
        if start.get("method") != Some(&json!("turn/start")) {
            return Err("wake delivery did not use the saved auto mode".into());
        }
        let correlation = start
            .pointer("/params/clientUserMessageId")
            .and_then(Value::as_str)
            .ok_or("wake push correlation missing")?
            .to_owned();
        socket
            .send(Message::Text(
                json!({"id":start["id"],"result":{"turn":{"id":"fixture-turn"}}})
                    .to_string()
                    .into(),
            ))
            .await?;
        correlation_sender
            .send(correlation)
            .map_err(|_| "wake correlation receiver dropped")?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });
    Ok(WakeAcceptanceFixture {
        native,
        correlation: correlation_receiver,
    })
}
