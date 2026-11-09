//! Stock 0.166.0 model requests distinguish a default tier from a requested effect.
use super::test_support::ScriptedSessionBackend;
use super::*;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

async fn fixture() -> TestResult<(
    Arc<ScriptedSessionBackend>,
    Arc<RouterSessionAppServerContext>,
)> {
    let backend = ScriptedSessionBackend::new()?;
    let catalog = vec![ProviderModelEntry::try_new(
        "new".into(),
        "New".into(),
        String::new(),
    )?];
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        backend.clone(),
        backend.clone(),
        test_support::model_catalog(tokio::sync::watch::channel(catalog).1),
    ));
    let started = handle_app_server_rpc(
        context.clone(),
        json!({"id":1,"method":"thread/start","params":{"cwd":"/tmp"}}),
    )
    .await?;
    if started.response.get("error").is_some() {
        return Err("fixture start failed".into());
    }
    Ok((backend, context))
}

fn model_picker_params(thread: String) -> Value {
    // Pinned ThreadSettingsUpdateParams uses null for ordinary absent fields;
    // the live 0.166.0 finding supplies serviceTier=default and effort=provider.
    json!({
        "threadId":thread,"model":"new","serviceTier":"default","effort":"provider",
        "collaborationMode":{"mode":"default","settings":{
            "model":"new","reasoning_effort":"provider","developer_instructions":null
        }},
        "disabledPluginIds":null,"cwd":null,"approvalPolicy":null,"approvalsReviewer":null,
        "sandboxPolicy":null,"permissions":null,"summary":null,"multiAgentMode":null,"personality":null
    })
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Protocol assertions; fixture and dispatch errors propagate."
)]
async fn stock_model_picker_default_null_and_omitted_service_tier_apply_only_the_model()
-> TestResult {
    for tier in [Some(json!("default")), Some(Value::Null), None] {
        // Arrange: model-only provider, matching the real CLI's model-picker payload.
        let (backend, context) = fixture().await?;
        let mut params = model_picker_params(thread_alias(&backend.session));
        match tier {
            Some(value) => params["serviceTier"] = value,
            None => {
                params
                    .as_object_mut()
                    .ok_or("params")?
                    .remove("serviceTier");
            }
        }
        // Act.
        let handled = handle_app_server_rpc(
            context,
            json!({"id":2,"method":"thread/settings/update","params":params}),
        )
        .await?;
        // Assert: success and the real model write, without a fake tier/effort command.
        assert_eq!(handled.response, json!({"id":2,"result":{}}));
        assert_eq!(
            *backend.effective_model.lock().expect("model"),
            Some("new".into())
        );
        let commands = backend.setting_commands.lock().expect("commands");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].setting_id, "model");
        assert_eq!(commands[0].value, "new");
        assert!(backend.effective_effort.lock().expect("effort").is_none());
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Protocol assertions; fixture and dispatch errors propagate."
)]
async fn non_default_service_tier_remains_unsupported_before_any_model_effect() -> TestResult {
    for tier in [
        json!("fast"),
        json!("priority"),
        json!(""),
        json!(false),
        json!(7),
        json!({}),
    ] {
        // Arrange / Act: the same model change with an unsupported tier request.
        let (backend, context) = fixture().await?;
        let mut params = model_picker_params(thread_alias(&backend.session));
        params["serviceTier"] = tier;
        let handled = handle_app_server_rpc(
            context,
            json!({"id":3,"method":"thread/settings/update","params":params}),
        )
        .await?;
        // Assert: refusal remains honest and happens before any provider write.
        assert_eq!(handled.response["error"]["code"], -32000);
        assert!(
            backend
                .setting_commands
                .lock()
                .expect("commands")
                .is_empty()
        );
        assert!(backend.effective_model.lock().expect("model").is_none());
    }
    Ok(())
}
