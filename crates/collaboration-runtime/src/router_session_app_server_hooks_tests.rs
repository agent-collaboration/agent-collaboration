//! Native hook discovery refuses unsupported inventory without provider effects.
use super::test_support::ScriptedSessionBackend;
use super::*;
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio_tungstenite::client_async;

/// Oracle: R3 requires an honest refusal when no hook discovery exists;
/// an empty success would be displayed as a real empty hook inventory.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Literal wire and zero-effect contract asserts; fixture errors propagate."
)]
async fn hooks_discovery_refuses_without_effects_and_keeps_the_connection_usable()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange one real native WebSocket with the existing effect-recording backend.
    let directory = tempfile::tempdir()?;
    let socket_path = directory.path().join("claude.sock");
    let listener = UnixListener::bind(&socket_path)?;
    let backend = ScriptedSessionBackend::new()?;
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        backend.clone(),
        backend.clone(),
        super::test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
    ));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        serve_router_session_app_server_connection(stream, context).await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });
    let stream = UnixStream::connect(&socket_path).await?;
    let (mut client, _) = client_async("ws://localhost/rpc", stream).await?;

    // Act through the same hooks/list request shape used by the native consumer.
    client
        .send(Message::Text(
            json!({"id":"hooks-41","method":"hooks/list","params":{"cwds":["/tmp"]}})
                .to_string()
                .into(),
        ))
        .await?;
    let reply = super::test_support::next_non_skill_frame(&mut client)
        .await
        .ok_or("missing hook discovery refusal")??;
    let refusal: Value = serde_json::from_str(reply.to_text()?)?;

    // Assert the correlated refusal, rather than a fabricated inventory result.
    assert_eq!(
        refusal,
        json!({"id":"hooks-41","error":{"code":-32000,"message":"Hook discovery is not supported on this ACP endpoint"}})
    );
    assert!(
        backend
            .create_commands
            .lock()
            .expect("create commands")
            .is_empty()
    );
    assert!(
        backend
            .prompt_commands
            .lock()
            .expect("prompt commands")
            .is_empty()
    );
    assert!(
        backend
            .setting_commands
            .lock()
            .expect("setting commands")
            .is_empty()
    );

    client
        .send(Message::Text(
            json!({"id":"read-42","method":"account/read","params":{}})
                .to_string()
                .into(),
        ))
        .await?;
    let reply = super::test_support::next_non_skill_frame(&mut client)
        .await
        .ok_or("missing supported read after hook refusal")??;
    let read: Value = serde_json::from_str(reply.to_text()?)?;
    assert_eq!(
        read,
        json!({"id":"read-42","result":{"account":null,"requiresOpenaiAuth":false}})
    );
    assert!(
        backend
            .create_commands
            .lock()
            .expect("create commands")
            .is_empty()
    );
    assert!(
        backend
            .prompt_commands
            .lock()
            .expect("prompt commands")
            .is_empty()
    );
    assert!(
        backend
            .setting_commands
            .lock()
            .expect("setting commands")
            .is_empty()
    );
    client.close(None).await?;
    server.await?.map_err(|error| error.to_string())?;
    Ok(())
}
