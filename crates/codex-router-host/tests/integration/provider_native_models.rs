//! Controlled ACP wire through real Host composition, native face and actor.
use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};
use tokio::net::UnixStream;
use tokio_tungstenite::{WebSocketStream, client_async, tungstenite::Message};

async fn rpc(
    client: &mut WebSocketStream<UnixStream>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    client
        .send(Message::Text(
            json!({"id":id,"method":method,"params":params})
                .to_string()
                .into(),
        ))
        .await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let frame = client.next().await.ok_or("closed native socket")??;
            let value: Value = serde_json::from_str(frame.to_text()?)?;
            if value.get("id") == Some(&json!(id)) {
                return Ok(value);
            }
        }
    })
    .await?
}
async fn wait_terminal(
    client: &mut WebSocketStream<UnixStream>,
    turn_id: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let frame = client.next().await.ok_or("missing terminal")??;
            let value: Value = serde_json::from_str(frame.to_text()?)?;
            if value.get("method") == Some(&json!("turn/completed"))
                && value.pointer("/params/turn/id") == Some(turn_id)
            {
                return Ok(());
            }
        }
    })
    .await?
}

async fn start_model_fixture(
    root: &std::path::Path,
    receipt: &std::path::Path,
    scenario: &str,
) -> Result<CollaborationRuntime, Box<dyn std::error::Error>> {
    let runtime = CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            board_session: None,
            owner_human_id: Some(message_board::HumanId::try_from("model-owner".to_owned())?),
            directory: root.into(),
            codex_home: root.into(),
            backend_socket: root.join("backend.sock"),
            mcp_bind: ([127, 0, 0, 1], 0).into(),
            native_schema: None,
            peer_registry_directory: None,
            remote_control_server_name: None,
        },
        vec![ExternalProviderStartup::Launch(
            ExternalProviderLaunchBinding::cursor(
                PathBuf::from("node"),
                vec![
                    concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../acp-client-runtime/tests/support/behavior-agent.ts"
                    )
                    .into(),
                    receipt.display().to_string(),
                    scenario.into(),
                    "cursor_login".into(),
                ],
            )?,
        )],
    )
    .await?;
    Ok(runtime)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Real two-session shared-peer counterexample; setup errors propagate."
)]
async fn cursor_shared_model_change_requires_restoring_another_sessions_cached_context_before_prompt()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let receipt = root.path().join("shared-settings.jsonl");
    let runtime = start_model_fixture(root.path(), &receipt, "settings").await?;
    let (mut client, _) = client_async(
        "ws://localhost/rpc",
        UnixStream::connect(root.path().join("router-sessions/cursor-local.sock")).await?,
    )
    .await?;
    let first = rpc(&mut client, 1, "thread/start", json!({"cwd":root.path()})).await?;
    let second = rpc(&mut client, 2, "thread/start", json!({"cwd":root.path()})).await?;
    let first_thread = first.pointer("/result/thread/id").ok_or("first thread")?;
    let second_thread = second.pointer("/result/thread/id").ok_or("second thread")?;
    let changed = rpc(
        &mut client,
        3,
        "thread/settings/update",
        json!({"threadId":second_thread,"model":"new","effort":"high"}),
    )
    .await?;
    assert!(changed.get("result").is_some());
    let turn = rpc(&mut client, 4, "turn/start", json!({"threadId":first_thread,"collaborationMode":{"mode":"default","settings":{"model":"old","reasoning_effort":"low","developer_instructions":null}},"input":[{"type":"text","text":"use restored context"}]})).await?;
    wait_terminal(&mut client, turn.pointer("/result/turn/id").ok_or("turn")?).await?;
    let wire = std::fs::read_to_string(&receipt)?
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    client.close(None).await?;
    runtime.shutdown().await?;
    assert_eq!(
        wire.len(),
        5,
        "session A must restore model and effort after B changes their shared provider"
    );
    assert_eq!(wire[0]["params"]["sessionId"], "session-2");
    assert_eq!(wire[0]["params"]["value"], "new");
    assert_eq!(wire[1]["params"]["value"], "high");
    assert_eq!(
        wire[2]["params"],
        json!({"sessionId":"session-1","configId":"model","value":"old"})
    );
    assert_eq!(
        wire[3]["params"],
        json!({"sessionId":"session-1","configId":"effort","value":"low"})
    );
    assert_eq!(wire[4]["method"], "session/prompt");
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "An unanswered optional request must remain bounded and leave ordinary work usable."
)]
async fn unanswered_optional_model_discovery_falls_back_once_and_preserves_session_models_and_chat()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let receipt = root.path().join("held-models.jsonl");
    let runtime = tokio::time::timeout(
        Duration::from_secs(5),
        start_model_fixture(root.path(), &receipt, "held-model-catalog"),
    )
    .await??;
    let (mut client, _) = client_async(
        "ws://localhost/rpc",
        UnixStream::connect(root.path().join("router-sessions/cursor-local.sock")).await?,
    )
    .await?;
    let created = rpc(&mut client, 1, "thread/start", json!({"cwd":root.path()})).await?;
    let thread = created
        .pointer("/result/thread/id")
        .ok_or("created thread")?;
    let models = rpc(&mut client, 2, "model/list", json!({})).await?;
    let rows = models["result"]["data"]
        .as_array()
        .ok_or("actual model rows")?;
    assert_eq!(
        rows.iter().map(|row| row["id"].clone()).collect::<Vec<_>>(),
        vec![json!("old"), json!("new")]
    );
    assert!(
        rows.iter()
            .all(|row| row["defaultReasoningEffort"] == "provider")
    );
    let turn = rpc(&mut client, 3, "turn/start", json!({"threadId":thread,"input":[{"type":"text","text":"ordinary after optional timeout"}]})).await?;
    wait_terminal(
        &mut client,
        turn.pointer("/result/turn/id").ok_or("ordinary turn")?,
    )
    .await?;
    let wire = std::fs::read_to_string(&receipt)?
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        wire.iter()
            .filter(|request| request["method"] == "cursor/list_available_models")
            .count(),
        1,
        "no optional read retry"
    );
    assert_eq!(
        wire.iter()
            .filter(|request| request["method"] == "session/set_config_option")
            .count(),
        0
    );
    assert_eq!(
        wire.iter()
            .filter(|request| request["method"] == "session/prompt")
            .count(),
        1
    );
    assert_eq!(
        wire.last()
            .and_then(|request| request.pointer("/params/sessionId")),
        Some(&json!("session-1")),
        "no hidden discovery Session"
    );
    client.close(None).await?;
    tokio::time::timeout(Duration::from_secs(5), runtime.shutdown()).await??;
    Ok(())
}

#[tokio::test]
async fn native_primary_enter_provider_settings_does_not_write_inherited_effort()
-> Result<(), Box<dyn std::error::Error>> {
    assert_primary_enter_model_selection("primary-enter", None).await
}

#[tokio::test]
async fn native_primary_enter_select_unoffered_hint_does_not_write_inherited_effort()
-> Result<(), Box<dyn std::error::Error>> {
    assert_primary_enter_model_selection("primary-enter-select", None).await
}

#[tokio::test]
async fn native_primary_enter_select_offered_hint_retains_actual_effort_write()
-> Result<(), Box<dyn std::error::Error>> {
    assert_primary_enter_model_selection("primary-enter-select-offered", Some("high")).await
}

async fn assert_primary_enter_model_selection(
    scenario: &str,
    first_effort_write: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let receipt = root.path().join("primary-enter.jsonl");
    let runtime = start_model_fixture(root.path(), &receipt, scenario).await?;
    let (mut client, _) = client_async(
        "ws://localhost/rpc",
        UnixStream::connect(root.path().join("router-sessions/cursor-local.sock")).await?,
    )
    .await?;
    let created = rpc(&mut client, 1, "thread/start", json!({"cwd":root.path()})).await?;
    let thread = created
        .pointer("/result/thread/id")
        .ok_or("created thread")?;
    let models = rpc(&mut client, 2, "model/list", json!({})).await?;
    let update_model = json!({"threadId":thread,"model":"new","effort":null,"collaborationMode":{"mode":"default","settings":{"model":"new","reasoning_effort":"high","developer_instructions":null}}});
    let chosen_effort = if scenario == "primary-enter" {
        "provider"
    } else {
        "low"
    };
    let select_effort = json!({"threadId":thread,"model":null,"effort":chosen_effort,"collaborationMode":{"mode":"default","settings":{"model":"new","reasoning_effort":chosen_effort,"developer_instructions":null}}});
    let first = rpc(
        &mut client,
        3,
        "thread/settings/update",
        update_model.clone(),
    )
    .await?;
    let first_wire = read_model_wire(&receipt)?;
    let first_state: Value =
        serde_json::from_slice(&std::fs::read(format!("{}.state.json", receipt.display()))?)?;
    let first_readback = rpc(
        &mut client,
        40,
        "thread/resume",
        json!({"threadId":thread,"excludeTurns":true}),
    )
    .await?;
    let second = rpc(
        &mut client,
        4,
        "thread/settings/update",
        select_effort.clone(),
    )
    .await?;
    let pair_wire = read_model_wire(&receipt)?;
    let pair_state: Value =
        serde_json::from_slice(&std::fs::read(format!("{}.state.json", receipt.display()))?)?;
    let saved = rpc(&mut client, 41, "config/batchWrite", json!({"edits":[{"keyPath":"model","value":"new","mergeStrategy":"replace"},{"keyPath":"model_reasoning_effort","value":chosen_effort,"mergeStrategy":"replace"}],"reloadUserConfig":true})).await?;
    let after_save_wire = read_model_wire(&receipt)?;
    let turn = rpc(&mut client, 5, "turn/start", json!({"threadId":thread,"collaborationMode":select_effort.get("collaborationMode").ok_or("selected mode")?,"input":[{"type":"text","text":"ordinary after selected provider model"}]})).await?;
    wait_terminal(
        &mut client,
        turn.pointer("/result/turn/id").ok_or("ordinary turn")?,
    )
    .await?;
    let wire = read_model_wire(&receipt)?;
    let state: Value =
        serde_json::from_slice(&std::fs::read(format!("{}.state.json", receipt.display()))?)?;
    client.close(None).await?;
    runtime.shutdown().await?;

    eprintln!(
        "PRIMARY_ENTER_EVIDENCE {}",
        json!({"scenario":scenario,"nativeSettingsRequests":[update_model,select_effort],"nativeSettingsResponses":[first,second],"firstProviderWire":first_wire,"firstProviderState":first_state,"firstReadback":first_readback,"pairProviderWire":pair_wire,"pairProviderState":pair_state,"providerWire":wire,"providerState":state})
    );
    assert_eq!(
        recorded_text(&created, "/result/reasoningEffort"),
        Some("high")
    );
    let target = models
        .pointer("/result/data")
        .ok_or("model data")?
        .as_array()
        .ok_or("models")?
        .iter()
        .find(|model| recorded_text(model, "/id") == Some("new"))
        .ok_or("new model")?;
    assert_eq!(
        recorded_text(target, "/defaultReasoningEffort"),
        Some(if scenario == "primary-enter" {
            "provider"
        } else {
            "low"
        })
    );
    assert!(first.get("result").is_some(), "first model update: {first}");
    let expected_first_effort = first_effort_write.unwrap_or("low");
    assert_eq!(recorded_text(&first_state, "/model"), Some("new"));
    assert_eq!(
        recorded_text(&first_state, "/effort"),
        Some(expected_first_effort)
    );
    assert_eq!(
        recorded_text(&first_state, "/savedEfforts/new"),
        Some(expected_first_effort)
    );
    assert_eq!(
        recorded_text(&first_state, "/savedEfforts/old"),
        Some("high")
    );
    assert_eq!(recorded_text(&first_readback, "/result/model"), Some("new"));
    assert_eq!(
        recorded_text(&first_readback, "/result/reasoningEffort"),
        Some(expected_first_effort)
    );
    assert_eq!(
        first_wire.len(),
        1 + usize::from(first_effort_write.is_some())
    );
    assert_eq!(
        recorded_text(
            first_wire.first().ok_or("first model write")?,
            "/params/configId"
        ),
        Some("model")
    );
    assert_eq!(
        recorded_text(
            first_wire.first().ok_or("first model write")?,
            "/params/value"
        ),
        Some("new")
    );
    if let Some(effort) = first_effort_write {
        assert_eq!(
            recorded_text(
                first_wire.get(1).ok_or("first effort write")?,
                "/params/configId"
            ),
            Some("effort")
        );
        assert_eq!(
            recorded_text(
                first_wire.get(1).ok_or("first effort write")?,
                "/params/value"
            ),
            Some(effort)
        );
    }
    assert!(second.get("result").is_some(), "{second}");
    let count = |requests: &[Value], setting: &str| {
        requests
            .iter()
            .filter(|request| {
                recorded_text(request, "/method") == Some("session/set_config_option")
                    && recorded_text(request, "/params/configId") == Some(setting)
            })
            .count()
    };
    assert_eq!(
        count(&pair_wire, "model"),
        2,
        "count model phases before the next turn"
    );
    assert_eq!(
        count(&pair_wire, "effort"),
        if scenario == "primary-enter" {
            0
        } else {
            1 + usize::from(first_effort_write.is_some())
        }
    );
    assert_eq!(recorded_text(&pair_state, "/effort"), Some("low"));
    assert_eq!(recorded_text(&pair_state, "/savedEfforts/new"), Some("low"));
    assert_eq!(
        saved,
        json!({"id":41,"result":{"status":"ok","version":"acp-compat-noop-v1","filePath":"/dev/null","overriddenMetadata":null}})
    );
    assert_eq!(
        after_save_wire, pair_wire,
        "default save must add no setting effects"
    );
    assert_eq!(
        count(&wire, "model"),
        3,
        "the next turn reasserts Cursor's shared model separately"
    );
    assert_eq!(
        wire.iter()
            .filter(|request| recorded_text(request, "/method") == Some("session/prompt"))
            .count(),
        1
    );
    if scenario == "primary-enter" {
        assert_eq!(count(&wire, "effort"), 0);
    } else if first_effort_write.is_none() {
        assert!(
            wire.iter()
                .filter(|request| recorded_text(request, "/params/configId") == Some("effort"))
                .all(|request| recorded_text(request, "/params/value") == Some("low"))
        );
    }
    assert_eq!(recorded_text(&state, "/model"), Some("new"));
    assert_eq!(recorded_text(&state, "/effort"), Some("low"));
    assert_eq!(recorded_text(&state, "/savedEfforts/new"), Some("low"));
    Ok(())
}

fn recorded_text<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn read_model_wire(path: &std::path::Path) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    Ok(std::fs::read_to_string(path)?
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn connection_descriptors_reach_initial_native_catalog_and_cursor_context_is_reasserted()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let receipt = root.path().join("models.jsonl");
    let runtime = start_model_fixture(root.path(), &receipt, "model-catalog").await?;
    let (mut client, _) = client_async(
        "ws://localhost/rpc",
        UnixStream::connect(root.path().join("router-sessions/cursor-local.sock")).await?,
    )
    .await?;
    let bootstrap = rpc(&mut client, 1, "model/list", json!({})).await?;
    assert_eq!(bootstrap["result"]["data"][0]["id"], "provider-default");
    let created = rpc(
        &mut client,
        2,
        "thread/start",
        json!({"cwd":root.path(),"model":"new"}),
    )
    .await?;
    let thread = created
        .pointer("/result/thread/id")
        .ok_or("created thread")?
        .clone();
    assert_eq!(created["result"]["thread"]["model"], "new");
    let models = rpc(&mut client, 3, "model/list", json!({})).await?;
    let rows = models["result"]["data"].as_array().ok_or("model rows")?;
    assert_eq!(
        rows.len(),
        3,
        "foreign extension rows never add session models"
    );
    assert_eq!(rows[0]["id"], "old");
    assert_eq!(rows[0]["defaultReasoningEffort"], "provider");
    assert_eq!(rows[1]["id"], "new");
    assert_eq!(rows[2]["id"], "rejected");
    assert_eq!(rows[2]["defaultReasoningEffort"], "provider");
    assert_eq!(
        rows[1]["defaultReasoningEffort"], "high",
        "vendor descriptor default differs from live low"
    );
    assert_eq!(created["result"]["reasoningEffort"], "low");
    for id in [4, 5] {
        let turn = rpc(&mut client, id, "turn/start", json!({"threadId":thread,"collaborationMode":{"mode":"default","settings":{"model":"new","reasoning_effort":"high","developer_instructions":null}},"input":[{"type":"text","text":"ordinary"}]})).await?;
        wait_terminal(&mut client, turn.pointer("/result/turn/id").ok_or("turn")?).await?;
    }
    let wire = std::fs::read_to_string(&receipt)?
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        wire.iter()
            .filter(|row| row["method"] == "cursor/list_available_models")
            .count(),
        1
    );
    let writes = wire
        .iter()
        .filter(|row| row["method"] == "session/set_config_option")
        .map(|row| row["params"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        writes,
        vec![
            json!({"sessionId":"session-1","configId":"model","value":"new"}),
            json!({"sessionId":"session-1","configId":"model","value":"new"}),
            json!({"sessionId":"session-1","configId":"effort","value":"high"}),
            json!({"sessionId":"session-1","configId":"model","value":"new"}),
            json!({"sessionId":"session-1","configId":"effort","value":"high"})
        ]
    );
    assert_eq!(
        wire.iter()
            .filter(|row| row["method"] == "session/prompt")
            .count(),
        2
    );
    let invalid = rpc(
        &mut client,
        6,
        "thread/start",
        json!({"cwd":root.path(),"model":"missing"}),
    )
    .await?;
    assert_eq!(invalid["error"]["code"], -32602);
    assert_eq!(invalid["error"]["data"]["kind"], "InvalidSetting");
    assert_eq!(invalid["error"]["data"]["target"]["sessionId"], "session-2");
    assert_eq!(
        invalid["error"]["data"]["detail"]["sessionDisposition"],
        "remainsCreated"
    );
    let retained = rpc(&mut client, 7, "model/list", json!({})).await?;
    assert_eq!(
        retained["result"]["data"][1]["id"], "new",
        "failed create must not bind the new session"
    );
    let partial = rpc(
        &mut client,
        8,
        "thread/start",
        json!({"cwd":root.path(),"model":"rejected"}),
    )
    .await?;
    assert_eq!(partial["error"]["code"], -32000);
    assert_eq!(partial["error"]["data"]["kind"], "CreatedWithoutSettings");
    assert_eq!(partial["error"]["data"]["target"]["sessionId"], "session-3");
    assert_eq!(
        partial["error"]["data"]["sessionDisposition"],
        "remainsCreated"
    );
    assert_eq!(partial["error"]["data"]["applied"], json!([]));
    assert_eq!(partial["error"]["data"]["failed"][0]["value"], "rejected");
    client.close(None).await?;
    runtime.shutdown().await?;
    Ok(())
}
