//! Native model/effort requests keep the pinned CLI's precedence and failure outcomes.
use super::test_support::ScriptedSessionBackend;
use super::*;
use collaboration_contracts::CommandFailure;

struct SettingsFixture {
    backend: Arc<ScriptedSessionBackend>,
    context: Arc<RouterSessionAppServerContext>,
    thread: String,
}

async fn setup() -> Result<SettingsFixture, Box<dyn std::error::Error>> {
    let backend = ScriptedSessionBackend::new()?;
    let catalog = vec![
        ProviderModelEntry::try_new("old".into(), "Old".into(), String::new())?,
        ProviderModelEntry::try_new("new".into(), "New".into(), String::new())?,
    ];
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
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        actor,
        backend.clone(),
        backend.clone(),
        tokio::sync::watch::channel(catalog).1,
    ));
    Ok(SettingsFixture {
        backend,
        context,
        thread: started["thread"]["id"]
            .as_str()
            .ok_or("thread ID")?
            .to_owned(),
    })
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions reject wrong effects; setup errors propagate."
)]
async fn settings_rpc_uses_default_mask_model_and_effort_before_acknowledging_actual_state()
-> Result<(), Box<dyn std::error::Error>> {
    let SettingsFixture {
        backend,
        context,
        thread,
    } = setup().await?;
    let handled = handle_app_server_rpc(context, json!({"id":1,"method":"thread/settings/update","params":{"threadId":thread,"model":"new","effort":"high","collaborationMode":{"mode":"default","settings":{"model":"old","reasoning_effort":"low","developer_instructions":null}}}})).await?;
    assert_eq!(handled.response, json!({"id":1,"result":{}}));
    let writes = backend.setting_commands.lock().expect("writes");
    assert_eq!(
        writes
            .iter()
            .map(|command| (command.setting_id.as_str(), command.value.as_str()))
            .collect::<Vec<_>>(),
        vec![("model", "old"), ("effort", "low")]
    );
    assert_eq!(
        backend.effective_model.lock().expect("model").as_deref(),
        Some("old")
    );
    assert_eq!(
        backend.effective_effort.lock().expect("effort").as_deref(),
        Some("low")
    );
    assert!(backend.prompt_commands.lock().expect("prompts").is_empty());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions reject wrong effects; setup errors propagate."
)]
async fn unsupported_native_effects_and_invalid_models_are_refused_before_settings_writes()
-> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (json!({"model":"missing"}), -32602),
        (
            json!({"model":"new","collaborationMode":{"mode":"plan","settings":{"model":"new"}}}),
            -32000,
        ),
        (
            json!({"model":"new","collaborationMode":{"mode":"default","settings":{"model":"new","developer_instructions":"custom instructions"}}}),
            -32000,
        ),
        (
            json!({"model":"new","permissions":"workspace-write"}),
            -32000,
        ),
        (json!({"model":"new","approvalPolicy":"never"}), -32000),
    ];
    for (mut params, code) in cases {
        let SettingsFixture {
            backend,
            context,
            thread,
        } = setup().await?;
        params
            .as_object_mut()
            .ok_or("params")?
            .insert("threadId".into(), json!(thread));
        let handled = handle_app_server_rpc(
            context,
            json!({"id":1,"method":"thread/settings/update","params":params}),
        )
        .await?;
        assert_eq!(
            handled.response["error"]["code"], code,
            "{}",
            handled.response
        );
        assert!(backend.setting_commands.lock().expect("writes").is_empty());
        assert!(backend.prompt_commands.lock().expect("prompts").is_empty());
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions preserve known partial effects; setup errors propagate."
)]
async fn model_then_effort_failure_returns_error_without_rollback_or_a_false_success()
-> Result<(), Box<dyn std::error::Error>> {
    let SettingsFixture {
        backend,
        context,
        thread,
    } = setup().await?;
    backend.setting_failures.lock().expect("failures").insert(
        "effort".into(),
        CommandFailure::InvalidSetting {
            advertised: vec!["low".into()],
        },
    );
    let handled = handle_app_server_rpc(context.clone(), json!({"id":1,"method":"thread/settings/update","params":{"threadId":thread,"model":"new","effort":"high"}})).await?;
    assert_eq!(handled.response["error"]["code"], -32602);
    assert_eq!(
        backend.effective_model.lock().expect("model").as_deref(),
        Some("new")
    );
    assert_eq!(
        backend.effective_effort.lock().expect("effort").as_deref(),
        None
    );
    let read = handle_app_server_rpc(
        context,
        json!({"id":2,"method":"thread/read","params":{"threadId":thread}}),
    )
    .await?;
    assert_eq!(read.response["result"]["thread"]["model"], "new");
    assert_eq!(backend.setting_commands.lock().expect("writes").len(), 2);
    assert!(backend.prompt_commands.lock().expect("prompts").is_empty());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions reject wrong precedence and synthetic mode writes; setup errors propagate."
)]
async fn turn_start_uses_mask_precedence_and_ordinary_absent_mask_keeps_input()
-> Result<(), Box<dyn std::error::Error>> {
    for mask in [
        None,
        Some(
            json!({"mode":"default","settings":{"model":"old","reasoning_effort":"low","developer_instructions":null}}),
        ),
    ] {
        let SettingsFixture {
            backend,
            context,
            thread,
        } = setup().await?;
        let expected = if mask.is_some() {
            [("model", "old"), ("effort", "low")]
        } else {
            [("model", "new"), ("effort", "high")]
        };
        let handled = handle_app_server_rpc(context, json!({"id":1,"method":"turn/start","params":{"threadId":thread,"model":"new","effort":"high","collaborationMode":mask,"cwd":"/tmp","runtimeWorkspaceRoots":[],"approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"workspaceWrite","writableRoots":[],"networkAccess":false,"excludeTmpdirEnvVar":false,"excludeSlashTmp":false},"permissions":null,"disabledPluginIds":null,"clientUserMessageId":"pinned-normal-turn","turnTrigger":"user","serviceTier":null,"input":[{"type":"text","text":"ordinary $text"}]}})).await?;
        assert!(
            handled.response.get("result").is_some(),
            "{}",
            handled.response
        );
        let settings = backend.setting_commands.lock().expect("settings");
        assert_eq!(
            settings
                .iter()
                .map(|command| (command.setting_id.as_str(), command.value.as_str()))
                .collect::<Vec<_>>(),
            expected
        );
        let prompts = backend.prompt_commands.lock().expect("prompts");
        assert_eq!(prompts.len(), 1);
        assert_eq!(
            prompts[0].submission.content,
            vec![CommandContent::text("ordinary $text".into())?]
        );
        assert!(prompts[0].submission.selected.is_empty());
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions reject a false reset success; setup errors propagate."
)]
async fn complete_default_mask_reset_is_unsupported_before_any_effect_but_sparse_null_preserves_effort()
-> Result<(), Box<dyn std::error::Error>> {
    for method in ["thread/settings/update", "turn/start"] {
        for nested_effort in [None, Some(Value::Null)] {
            let SettingsFixture {
                backend,
                context,
                thread,
            } = setup().await?;
            let established = handle_app_server_rpc(context.clone(), json!({"id":1,"method":"thread/settings/update","params":{"threadId":thread,"model":"new","effort":"high"}})).await?;
            assert!(established.response.get("result").is_some());
            backend.setting_commands.lock().expect("writes").clear();
            let mut mode_settings = json!({"model":"old","developer_instructions":null});
            if let Some(effort) = nested_effort {
                mode_settings
                    .as_object_mut()
                    .ok_or("mode settings")?
                    .insert("reasoning_effort".into(), effort);
            }
            let refused = handle_app_server_rpc(context.clone(), json!({"id":2,"method":method,"params":{"threadId":thread,"collaborationMode":{"mode":"default","settings":mode_settings},"input":[{"type":"text","text":"must not send"}]}})).await?;
            assert_eq!(
                refused.response["error"]["code"], -32000,
                "{}",
                refused.response
            );
            assert!(backend.setting_commands.lock().expect("writes").is_empty());
            assert!(backend.prompt_commands.lock().expect("prompts").is_empty());
            assert_eq!(
                backend.effective_model.lock().expect("model").as_deref(),
                Some("new")
            );
            assert_eq!(
                backend.effective_effort.lock().expect("effort").as_deref(),
                Some("high")
            );
            let preserved = handle_app_server_rpc(context, json!({"id":3,"method":method,"params":{"threadId":thread,"collaborationMode":null,"effort":null,"input":[{"type":"text","text":"preserve sparse settings"}]}})).await?;
            assert!(
                preserved.response.get("result").is_some(),
                "{}",
                preserved.response
            );
            assert!(backend.setting_commands.lock().expect("writes").is_empty());
            assert_eq!(
                backend.effective_effort.lock().expect("effort").as_deref(),
                Some("high")
            );
            assert_eq!(
                backend.prompt_commands.lock().expect("prompts").len(),
                usize::from(method == "turn/start")
            );
        }
    }
    Ok(())
}
