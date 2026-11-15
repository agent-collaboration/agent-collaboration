use super::test_support::ScriptedSessionBackend;
use super::*;
use serde_json::json;

/// Oracle: spec R1 "Those doors ask for auto by name". Native `thread/start` has no
/// field for a permission mode, so the app-server door names `auto` itself.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn thread_start_asks_for_auto() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange: a valid thread/start with a real working directory and no model.
    let directory = tempfile::tempdir()?;
    let backend = ScriptedSessionBackend::new()?;

    // Act: the native door creates the Session.
    let started = handle_app_server_thread_request(
        "thread/start",
        json!({"cwd":directory.path()}),
        Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(&backend) as Arc<dyn SessionEventHub>,
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        &[],
    )
    .await?;

    // Assert: the create succeeded and asked for mode auto; the model stays unset.
    assert_eq!(started["thread"]["cwd"], json!(directory.path()));
    let commands = backend.create_commands.lock().expect("test lock");
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].settings.mode.as_deref(), Some("auto"));
    assert_eq!(commands[0].settings.model, None);
    assert_eq!(commands[0].settings.effort, None);
    Ok(())
}
