//! Real native face, Host delivery, injected Cursor behavior and actor dispatch.
use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};
use tokio::net::UnixStream;
use tokio_tungstenite::{WebSocketStream, client_async, tungstenite::Message};
const BOUND: Duration = Duration::from_secs(5);

async fn send(
    client: &mut WebSocketStream<UnixStream>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<(), Box<dyn std::error::Error>> {
    client
        .send(Message::Text(
            json!({"id":id,"method":method,"params":params})
                .to_string()
                .into(),
        ))
        .await?;
    Ok(())
}
async fn reply(
    client: &mut WebSocketStream<UnixStream>,
    id: u64,
    notifications: &mut Vec<Value>,
) -> Result<Value, Box<dyn std::error::Error>> {
    tokio::time::timeout(BOUND, async {
        loop {
            let frame = client.next().await.ok_or("native socket closed")??;
            let value: Value = serde_json::from_str(frame.to_text()?)?;
            if value.get("id").is_some() {
                if value.get("id").and_then(Value::as_u64) != Some(id) {
                    return Err("wrong native correlation".into());
                }
                return Ok(value);
            }
            notifications.push(value);
        }
    })
    .await?
}
fn receipts(path: &std::path::Path) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    std::fs::read_to_string(path)?
        .lines()
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

async fn terminal(
    client: &mut WebSocketStream<UnixStream>,
    turn_id: &str,
    notifications: &mut Vec<Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(BOUND, async {
        loop {
            if notifications.iter().any(|value| {
                value.get("method").and_then(Value::as_str) == Some("turn/completed")
                    && value.pointer("/params/turn/id").and_then(Value::as_str) == Some(turn_id)
            }) {
                return Ok(());
            }
            let frame = client.next().await.ok_or("missing correlated terminal")??;
            notifications.push(serde_json::from_str(frame.to_text()?)?);
        }
    })
    .await?
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on incorrect wire behavior; fixture errors propagate."
)]
async fn native_selected_refusal_is_correlated_before_any_send_and_valid_input_keeps_exact_wire_content()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let receipt = root.path().join("wire.jsonl");
    let runtime = CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            board_session: None,
            owner_human_id: Some(message_board::HumanId::try_from(
                "native-skill-owner".to_owned(),
            )?),
            directory: root.path().to_owned(),
            codex_home: root.path().to_owned(),
            backend_socket: root.path().join("backend.sock"),
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
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
                    "manual".into(),
                ],
            )?,
        )],
    )
    .await?;
    let (mut client, _) = client_async(
        "ws://localhost/rpc",
        UnixStream::connect(root.path().join("router-sessions/cursor-local.sock")).await?,
    )
    .await?;
    let mut notifications = Vec::new();
    send(&mut client, 1, "thread/start", json!({"cwd":root.path()})).await?;
    let started = reply(&mut client, 1, &mut notifications).await?;
    let thread = started
        .pointer("/result/thread/id")
        .and_then(Value::as_str)
        .ok_or("native thread")?
        .to_owned();
    let mut sequence = 2;
    let selection = tokio::time::timeout(BOUND, async {
        loop {
            send(
                &mut client,
                sequence,
                "skills/list",
                json!({"cwds":[root.path()]}),
            )
            .await?;
            let listed = reply(&mut client, sequence, &mut notifications).await?;
            sequence += 1;
            if let Some(skill) = listed.pointer("/result/data/0/skills/0") {
                return Ok::<_, Box<dyn std::error::Error>>(skill.clone());
            }
            // A fresh list is requested only after the source's causal invalidation.
            loop {
                let frame = client
                    .next()
                    .await
                    .ok_or("missing metadata invalidation")??;
                let value: Value = serde_json::from_str(frame.to_text()?)?;
                let invalidation =
                    value.get("method").and_then(Value::as_str) == Some("skills/changed");
                notifications.push(value);
                if invalidation {
                    break;
                }
            }
        }
    })
    .await??;
    assert_eq!(selection["name"], "review");
    assert_eq!(selection["scope"], "system");
    let path = selection
        .get("path")
        .and_then(Value::as_str)
        .ok_or("selection path")?;
    assert_eq!(
        path,
        "/__codex_router_skill__/cursor-local/c2Vzc2lvbi0x/cmV2aWV3"
    );
    let selected = json!({"type":"skill","name":"review","path":path});
    notifications.clear();
    send(&mut client, sequence, "turn/start", json!({"threadId":thread,"input":[{"type":"text","text":"$review"},{"type":"text","text":"second"},selected]})).await?;
    let refused = reply(&mut client, sequence, &mut notifications).await?;
    sequence += 1;
    assert_eq!(refused["error"]["code"], -32000);
    assert!(receipts(&receipt)?.is_empty());
    assert!(
        !notifications
            .iter()
            .any(|notification| notification["method"] == "turn/started")
    );
    send(
        &mut client,
        sequence,
        "thread/compact/start",
        json!({"threadId":thread}),
    )
    .await?;
    assert_eq!(
        reply(&mut client, sequence, &mut notifications).await?["error"]["code"],
        -32000
    );
    sequence += 1;
    assert!(receipts(&receipt)?.is_empty());
    send(&mut client, sequence, "turn/start", json!({"threadId":thread,"input":[{"type":"text","text":"前置\t$review  args\n"},{"type":"image","url":"data:image/png;base64,aGVsbG8="},selected]})).await?;
    let accepted = reply(&mut client, sequence, &mut notifications).await?;
    sequence += 1;
    assert!(accepted.get("result").is_some(), "{accepted}");
    // Await this actual terminal before the next operation, rather than sleeping.
    terminal(
        &mut client,
        accepted
            .pointer("/result/turn/id")
            .and_then(Value::as_str)
            .ok_or("turn ID")?,
        &mut notifications,
    )
    .await?;
    let wire = receipts(&receipt)?;
    assert_eq!(wire.len(), 1);
    assert_eq!(
        wire[0]["params"]["prompt"],
        json!([{"type":"text","text":"前置\t/review  args\n"},{"type":"image","mimeType":"image/png","data":"aGVsbG8="}])
    );
    notifications.clear();
    send(
        &mut client,
        sequence,
        "turn/start",
        json!({"threadId":thread,"input":[{"type":"text","text":"remove-skill"}]}),
    )
    .await?;
    let removal = reply(&mut client, sequence, &mut notifications).await?;
    sequence += 1;
    terminal(
        &mut client,
        removal
            .pointer("/result/turn/id")
            .and_then(Value::as_str)
            .ok_or("removal turn ID")?,
        &mut notifications,
    )
    .await?;
    notifications.clear();
    send(
        &mut client,
        sequence,
        "turn/start",
        json!({"threadId":thread,"input":[{"type":"text","text":"$review"},selected]}),
    )
    .await?;
    assert_eq!(
        reply(&mut client, sequence, &mut notifications).await?["error"]["code"],
        -32602
    );
    assert_eq!(receipts(&receipt)?.len(), 2);
    assert!(
        !notifications
            .iter()
            .any(|notification| notification["method"] == "turn/started")
    );
    sequence += 1;
    send(
        &mut client,
        sequence,
        "turn/start",
        json!({"threadId":thread,"input":[{"type":"text","text":"after-refusal-barrier"}]}),
    )
    .await?;
    let barrier = reply(&mut client, sequence, &mut notifications).await?;
    terminal(
        &mut client,
        barrier
            .pointer("/result/turn/id")
            .and_then(Value::as_str)
            .ok_or("barrier turn ID")?,
        &mut notifications,
    )
    .await?;
    assert_eq!(
        receipts(&receipt)?.len(),
        3,
        "no hidden earlier submission before the same-provider barrier"
    );
    client.close(None).await?;
    tokio::time::timeout(BOUND, runtime.shutdown()).await??;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions reject wrong native/provider effects; setup errors propagate."
)]
async fn native_settings_reaches_provider_and_readback_retains_confirmed_partial_effects()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let receipt = root.path().join("settings.jsonl");
    let runtime = CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            board_session: None,
            owner_human_id: Some(message_board::HumanId::try_from(
                "native-settings-owner".to_owned(),
            )?),
            directory: root.path().to_owned(),
            codex_home: root.path().to_owned(),
            backend_socket: root.path().join("backend.sock"),
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
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
                    "settings".into(),
                ],
            )?,
        )],
    )
    .await?;
    let (mut client, _) = client_async(
        "ws://localhost/rpc",
        UnixStream::connect(root.path().join("router-sessions/cursor-local.sock")).await?,
    )
    .await?;
    let mut notifications = Vec::new();
    send(&mut client, 1, "thread/start", json!({"cwd":root.path()})).await?;
    let started = reply(&mut client, 1, &mut notifications).await?;
    let thread = started
        .pointer("/result/thread/id")
        .and_then(Value::as_str)
        .ok_or("thread ID")?;
    send(
        &mut client,
        2,
        "thread/settings/update",
        json!({"threadId":thread,"model":"new","effort":"high"}),
    )
    .await?;
    assert_eq!(
        reply(&mut client, 2, &mut notifications).await?,
        json!({"id":2,"result":{}})
    );
    send(&mut client, 3, "thread/read", json!({"threadId":thread})).await?;
    assert_eq!(
        reply(&mut client, 3, &mut notifications).await?["result"]["thread"]["model"],
        "new"
    );
    let writes = receipts(&receipt)?;
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0]["method"], "session/set_config_option");
    assert_eq!(writes[0]["params"]["configId"], "model");
    assert_eq!(writes[0]["params"]["value"], "new");
    assert_eq!(writes[1]["params"]["configId"], "effort");
    assert_eq!(writes[1]["params"]["value"], "high");
    for (index, mode_settings) in [
        json!({"model":"old","reasoning_effort":null,"developer_instructions":null}),
        json!({"model":"old","developer_instructions":null}),
    ]
    .into_iter()
    .enumerate()
    {
        let id = 20 + u64::try_from(index)?;
        send(&mut client, id, "thread/settings/update", json!({"threadId":thread,"collaborationMode":{"mode":"default","settings":mode_settings}})).await?;
        assert_eq!(
            reply(&mut client, id, &mut notifications).await?["error"]["code"],
            -32000
        );
        assert_eq!(
            receipts(&receipt)?.len(),
            2,
            "unqualified effort reset must have zero provider effects"
        );
    }
    // Switching back narrows the provider's advertised efforts. The next field
    // must validate against that fresh catalogue and retain the confirmed model.
    send(
        &mut client,
        4,
        "thread/settings/update",
        json!({"threadId":thread,"model":"old","effort":"high"}),
    )
    .await?;
    assert_eq!(
        reply(&mut client, 4, &mut notifications).await?["error"]["code"],
        -32602
    );
    send(&mut client, 5, "thread/read", json!({"threadId":thread})).await?;
    assert_eq!(
        reply(&mut client, 5, &mut notifications).await?["result"]["thread"]["model"],
        "old"
    );
    let writes = receipts(&receipt)?;
    assert_eq!(
        writes.len(),
        3,
        "invalid effort must never reach the provider"
    );
    assert_eq!(writes[2]["params"]["value"], "old");
    send(
        &mut client,
        6,
        "turn/start",
        json!({"threadId":thread,"input":[{"type":"text","text":"usable after partial refusal"}]}),
    )
    .await?;
    let turn = reply(&mut client, 6, &mut notifications).await?;
    terminal(
        &mut client,
        turn.pointer("/result/turn/id")
            .and_then(Value::as_str)
            .ok_or("turn")?,
        &mut notifications,
    )
    .await?;
    assert_eq!(receipts(&receipt)?.len(), 4);
    client.close(None).await?;
    runtime.shutdown().await?;
    Ok(())
}
