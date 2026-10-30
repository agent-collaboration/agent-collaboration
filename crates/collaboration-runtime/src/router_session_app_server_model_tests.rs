use super::test_support::ScriptedSessionBackend;
use super::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::{UnixListener, UnixStream};
use tokio_tungstenite::{client_async, tungstenite::Message};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn unmapped_model_refuses_before_creating_a_provider_session()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let socket_path = directory.path().join("claude.sock");
    let listener = UnixListener::bind(&socket_path)?;
    let backend = ScriptedSessionBackend::new()?;
    *backend.effective_model.lock().expect("test lock") = Some("default".into());
    let catalog = vec![
        ProviderModelEntry::try_new("default".into(), "Default".into(), String::new())?,
        ProviderModelEntry::try_new("sonnet".into(), "Sonnet".into(), String::new())?,
    ];
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(&backend) as Arc<dyn SessionEventHub>,
        tokio::sync::watch::channel(catalog).1,
    ));
    let serving = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        serve_router_session_app_server_connection(stream, context)
            .await
            .expect("serve request");
    });

    let stream = UnixStream::connect(&socket_path).await?;
    let (mut client, _) = client_async("ws://localhost/rpc", stream).await?;
    client
        .send(Message::Text(
            json!({"id":1,"method":"thread/start","params":{
                "cwd":"/tmp","model":"gpt-6-luna"
            }})
            .to_string()
            .into(),
        ))
        .await?;
    let response: Value = serde_json::from_str(client.next().await.ok_or("response")??.to_text()?)?;
    assert_eq!(response["error"]["code"], -32602);
    assert!(
        backend
            .create_commands
            .lock()
            .expect("test lock")
            .is_empty()
    );

    client.close(None).await?;
    serving.await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn turn_model_uses_only_provider_advertised_choices() -> Result<(), Box<dyn std::error::Error>>
{
    let catalog = vec![
        ProviderModelEntry::try_new("default".into(), "Default".into(), String::new())?,
        ProviderModelEntry::try_new("sonnet".into(), "Sonnet".into(), String::new())?,
    ];
    for requested_model in ["gpt-6-luna", "provider-default", "sonnet"] {
        let expected_override = (requested_model == "sonnet").then_some("sonnet");
        let backend = ScriptedSessionBackend::new()?;
        let actor = ScriptedSessionBackend::actor()?;
        let started = handle_app_server_thread_request(
            "thread/start",
            json!({"cwd":"/tmp","model":"provider-default"}),
            Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
            Arc::clone(&backend) as Arc<dyn SessionEventHub>,
            backend.endpoint.clone(),
            actor.clone(),
            &catalog,
        )
        .await?;
        let turn = handle_app_server_turn_request(
            "turn/start",
            json!({"threadId":started["thread"]["id"],
                "model":requested_model,
                "input":[{"type":"text","text":"hello"}]}),
            Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
            Arc::clone(&backend) as Arc<dyn SessionEventHub>,
            backend.endpoint.clone(),
            actor,
            &catalog,
        )
        .await;
        if requested_model == "gpt-6-luna" {
            assert!(matches!(turn, Err(ThreadMethodError::InvalidParams)));
            assert!(
                backend
                    .setting_commands
                    .lock()
                    .expect("test lock")
                    .is_empty()
            );
            assert!(
                backend
                    .prompt_commands
                    .lock()
                    .expect("test lock")
                    .is_empty()
            );
            continue;
        }
        let turn = turn?;
        assert_eq!(turn["turn"]["status"], "inProgress");
        let settings = backend.setting_commands.lock().expect("test lock");
        assert_eq!(settings.len(), usize::from(expected_override.is_some()));
        if let Some(expected) = expected_override {
            assert_eq!(settings[0].setting_id, "model");
            assert_eq!(settings[0].value, expected);
        }
        assert_eq!(backend.prompt_commands.lock().expect("test lock").len(), 1);
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions reject wrong effects; fixture errors propagate."
)]
async fn unmapped_native_plan_has_zero_effect_while_default_metadata_keeps_ordinary_chat()
-> Result<(), Box<dyn std::error::Error>> {
    let catalog = vec![ProviderModelEntry::try_new(
        "sonnet".into(),
        "Sonnet".into(),
        String::new(),
    )?];
    for mode in ["plan", "default"] {
        let backend = ScriptedSessionBackend::new()?;
        let actor = ScriptedSessionBackend::actor()?;
        let started = handle_app_server_thread_request(
            "thread/start",
            json!({"cwd":"/tmp"}),
            backend.clone(),
            backend.clone(),
            backend.endpoint.clone(),
            actor.clone(),
            &catalog,
        )
        .await?;
        let result = handle_app_server_turn_request("turn/start", json!({"threadId":started["thread"]["id"],"model":"sonnet","collaborationMode":{"mode":mode,"settings":{"model":"sonnet","reasoning_effort":"low","developer_instructions":null}},"input":[{"type":"text","text":"ordinary text"}]}), backend.clone(), backend.clone(), backend.endpoint.clone(), actor, &catalog).await;
        if mode == "plan" {
            assert!(matches!(result, Err(ThreadMethodError::Unsupported)));
            assert!(
                backend
                    .setting_commands
                    .lock()
                    .expect("settings")
                    .is_empty()
            );
            assert!(backend.prompt_commands.lock().expect("prompts").is_empty());
        } else {
            assert!(result.is_ok());
            let settings = backend.setting_commands.lock().expect("settings");
            assert_eq!(settings.len(), 2);
            assert_eq!(settings[0].setting_id, "model");
            assert_eq!(settings[0].value, "sonnet");
            assert_eq!(settings[1].setting_id, "effort");
            assert_eq!(settings[1].value, "low");
            assert_eq!(backend.prompt_commands.lock().expect("prompts").len(), 1);
        }
    }
    Ok(())
}
