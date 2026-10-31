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
    let catalog = vec![
        super::test_support::qualified_model("old", "Old", &["low", "high"], "low")?,
        super::test_support::qualified_model("new", "New", &["low", "high"], "low")?,
    ];
    setup_with_catalog(catalog).await
}

async fn setup_with_catalog(
    catalog: Vec<ProviderModelEntry>,
) -> Result<SettingsFixture, Box<dyn std::error::Error>> {
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
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        actor,
        backend.clone(),
        backend.clone(),
        super::test_support::model_catalog(tokio::sync::watch::channel(catalog).1),
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
    reason = "Refusal and supported sparse/context forms must retain their distinct effects."
)]
async fn unknown_model_numeric_action_refuses_whole_dedicated_request_without_changing_sparse_or_turn_context()
-> Result<(), Box<dyn std::error::Error>> {
    for (method, mut params, refused, setting_count) in [
        (
            "thread/settings/update",
            json!({"model":"new","effort":"high"}),
            true,
            0,
        ),
        (
            "thread/settings/update",
            json!({"collaborationMode":{"mode":"default","settings":{"model":"new","reasoning_effort":"high","developer_instructions":null}}}),
            true,
            0,
        ),
        ("thread/settings/update", json!({"effort":"high"}), false, 1),
        (
            "thread/settings/update",
            json!({"model":"provider-default","effort":"high"}),
            true,
            0,
        ),
        (
            "thread/settings/update",
            json!({"collaborationMode":{"mode":"default","settings":{"model":"provider-default","reasoning_effort":"high","developer_instructions":null}}}),
            true,
            0,
        ),
        (
            "thread/settings/update",
            json!({"model":"provider-default","effort":"provider"}),
            false,
            0,
        ),
        (
            "turn/start",
            json!({"model":"provider-default","effort":"high","input":[{"type":"text","text":"delegated model context"}]}),
            false,
            1,
        ),
        (
            "turn/start",
            json!({"model":"new","effort":"high","input":[{"type":"text","text":"explicit turn context"}]}),
            false,
            2,
        ),
    ] {
        let SettingsFixture {
            backend,
            context,
            thread,
        } = setup_with_catalog(vec![ProviderModelEntry::try_new(
            "new".into(),
            "New".into(),
            String::new(),
        )?])
        .await?;
        params
            .as_object_mut()
            .ok_or("params")?
            .insert("threadId".into(), json!(thread));
        let handled = handle_app_server_rpc(
            context,
            json!({"id":"scope-check","method":method,"params":params}),
        )
        .await?;
        assert_eq!(handled.response["id"], "scope-check");
        if refused {
            assert_eq!(handled.response["error"]["code"], -32000);
        } else {
            assert!(
                handled.response.get("result").is_some(),
                "{}",
                handled.response
            );
        }
        assert_eq!(
            backend.setting_commands.lock().expect("settings").len(),
            setting_count
        );
        assert_eq!(
            backend.prompt_commands.lock().expect("prompts").len(),
            usize::from(method == "turn/start")
        );
    }
    Ok(())
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
async fn unknown_complete_default_mask_delegates_effort_and_sparse_null_preserves_it()
-> Result<(), Box<dyn std::error::Error>> {
    for method in ["thread/settings/update", "turn/start"] {
        for nested_effort in [None, Some(Value::Null)] {
            let SettingsFixture {
                backend,
                context,
                thread,
            } = setup_with_catalog(vec![
                ProviderModelEntry::try_new("old".into(), "Old".into(), String::new())?,
                ProviderModelEntry::try_new("new".into(), "New".into(), String::new())?,
            ])
            .await?;
            let delegated = handle_app_server_rpc(context.clone(), json!({"id":0,"method":"thread/settings/update","params":{"threadId":thread,"model":"new","effort":"provider"}})).await?;
            assert!(delegated.response.get("result").is_some());
            let established = handle_app_server_rpc(context.clone(), json!({"id":1,"method":"thread/settings/update","params":{"threadId":thread,"effort":"high"}})).await?;
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
            assert!(
                refused.response.get("result").is_some(),
                "{}",
                refused.response
            );
            assert_eq!(backend.setting_commands.lock().expect("writes").len(), 1);
            assert_eq!(
                backend.setting_commands.lock().expect("writes")[0].setting_id,
                "model"
            );
            assert_eq!(
                backend.effective_model.lock().expect("model").as_deref(),
                Some("old")
            );
            assert_eq!(
                backend.effective_effort.lock().expect("effort").as_deref(),
                Some("high")
            );
            backend.setting_commands.lock().expect("writes").clear();
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
                2 * usize::from(method == "turn/start")
            );
        }
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions reject dropped current settings and false reset refusals; setup errors propagate."
)]
async fn start_and_resume_preserve_latest_exact_current_effort_in_their_atomic_attachment()
-> Result<(), Box<dyn std::error::Error>> {
    for effort in [Some("default"), Some("vendor:turbo"), Some("high"), None] {
        let backend = ScriptedSessionBackend::new()?;
        *backend.effective_model.lock().expect("model") = Some("old".into());
        *backend.effective_effort.lock().expect("effort") = effort.map(str::to_owned);
        backend
            .history
            .lock()
            .expect("history")
            .push(collaboration_contracts::HubEvent {
                sequence: 1,
                event: session_event_model::SessionEvent::SettingsChanged {
                    settings: session_event_model::SessionSettings {
                        model: Some("old".into()),
                        effort: effort.map(str::to_owned),
                        ..Default::default()
                    },
                },
            });
        let context = Arc::new(RouterSessionAppServerContext::new(
            backend.endpoint.clone(),
            ScriptedSessionBackend::actor()?,
            backend.clone(),
            backend.clone(),
            super::test_support::model_catalog(
                tokio::sync::watch::channel(vec![
                    super::test_support::qualified_model(
                        "old",
                        "Old",
                        &["default", "vendor:turbo", "high", "latest:exact"],
                        "default",
                    )?,
                    super::test_support::qualified_model(
                        "new",
                        "New",
                        &["default", "vendor:turbo", "high", "latest:exact"],
                        "default",
                    )?,
                ])
                .1,
            ),
        ));
        let started = handle_app_server_rpc(
            context.clone(),
            json!({"id":1,"method":"thread/start","params":{"cwd":"/tmp"}}),
        )
        .await?;
        assert_eq!(started.response["result"]["reasoningEffort"], json!(effort));
        assert!(
            started.session_attachment.is_some(),
            "start must retain the exact reply snapshot for subscription"
        );
        assert_eq!(
            backend
                .attachment_count
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        let thread = &started.response["result"]["thread"]["id"];
        if let Some(effort) = effort {
            // Pinned stock /model first sends UpdateModel with a complete Default
            // mask seeded from session.reasoning_effort, before its effort event.
            let changed = handle_app_server_rpc(context.clone(), json!({"id":2,"method":"thread/settings/update","params":{"threadId":thread,"model":"new","collaborationMode":{"mode":"default","settings":{"model":"new","reasoning_effort":effort,"developer_instructions":null}}}})).await?;
            assert_eq!(changed.response, json!({"id":2,"result":{}}));
            let writes = backend.setting_commands.lock().expect("writes");
            assert_eq!(writes.len(), 2);
            assert_eq!(writes[0].value, "new");
            assert_eq!(writes[1].value, effort);
        }
        let updated = handle_app_server_rpc(context.clone(), json!({"id":3,"method":"thread/settings/update","params":{"threadId":thread,"effort":"latest:exact"}})).await?;
        assert!(updated.response.get("result").is_some());
        let resumed = handle_app_server_rpc(context.clone(), json!({"id":4,"method":"thread/resume","params":{"threadId":thread,"excludeTurns":true}})).await?;
        assert_eq!(
            resumed.response["result"]["reasoningEffort"],
            "latest:exact"
        );
        assert!(resumed.session_attachment.is_some());
        assert_eq!(
            backend
                .attachment_count
                .load(std::sync::atomic::Ordering::Relaxed),
            2
        );
        backend
            .history
            .lock()
            .expect("history")
            .push(collaboration_contracts::HubEvent {
                sequence: 99,
                event: session_event_model::SessionEvent::SettingsChanged {
                    settings: Default::default(),
                },
            });
        let unknown = handle_app_server_rpc(
            context,
            json!({"id":5,"method":"thread/resume","params":{"threadId":thread}}),
        )
        .await?;
        assert!(
            unknown.response["result"]["reasoningEffort"].is_null(),
            "latest unknown must not fall back to an older effort"
        );
        assert!(backend.prompt_commands.lock().expect("prompts").is_empty());
    }
    Ok(())
}
