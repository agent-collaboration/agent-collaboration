//! Actual MCP → application → Host/runtime → external controlled ACP peer.
use super::*;
use reqwest::header::{ACCEPT, CONTENT_TYPE};

fn scenario_error(error: impl std::fmt::Display) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(error.to_string()).into()
}

async fn with_owned_host<TObservation>(
    runtime: CollaborationRuntime,
    observation: TObservation,
) -> TestResult
where
    TObservation:
        for<'runtime> std::ops::AsyncFnOnce(&'runtime mut CollaborationRuntime) -> TestResult,
{
    owned_process_test_support::scenario_finalization::run_scenario(
        runtime,
        async |runtime: &mut CollaborationRuntime| {
            observation(runtime).await.map_err(scenario_error)
        },
        async |runtime: CollaborationRuntime| runtime.shutdown().await.map_err(Into::into),
        || Ok(()),
    )
    .await
    .map_err(|error| std::io::Error::other(error.to_string()).into())
}

fn require_equal<TActual, TExpected>(
    actual: &TActual,
    expected: &TExpected,
    context: &str,
) -> TestResult
where
    TActual: PartialEq<TExpected> + std::fmt::Debug,
    TExpected: std::fmt::Debug,
{
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{context}: expected {expected:?}, observed {actual:?}").into())
    }
}

macro_rules! check_eq {
    ($actual:expr, $expected:expr $(,)?) => {
        require_equal(&$actual, &$expected, stringify!($actual))?
    };
    ($actual:expr, $expected:expr, $($context:tt)+) => {
        require_equal(&$actual, &$expected, &format!($($context)+))?
    };
}

macro_rules! check {
    ($condition:expr $(,)?) => {
        if !$condition { return Err(stringify!($condition).into()); }
    };
    ($condition:expr, $($context:tt)+) => {
        if !$condition { return Err(format!($($context)+).into()); }
    };
}

#[test]
fn controlled_behavior_peer_release_and_race_tests_are_discovered() {
    let result = std::process::Command::new(node_test_fixture::node_executable().expect("Node"))
        .args([
            "--test",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../acp-client-runtime/tests/support/behavior-agent.test.ts"
            ),
        ])
        .output()
        .expect("fixture tests execute");
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "{stdout}\n{stderr}");
    assert!(
        stdout
            .lines()
            .any(|line| matches!(line.trim(), "# tests 10" | "ℹ tests 10"))
            && stdout
                .lines()
                .any(|line| matches!(line.trim(), "# pass 10" | "ℹ pass 10")),
        "all ten permanent fixture cases must execute: {stdout}\n{stderr}"
    );
}

async fn mcp_call(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let response = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-11-25")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await
        .map_err(|error| format!("MCP HTTP response: {error}"))?;
    check!(response.status().is_success(), "{response:?}");
    let body = response
        .text()
        .await
        .map_err(|error| format!("MCP response body: {error}"))?;
    let frame = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .find(|line| !line.is_empty())
        .unwrap_or(&body);
    serde_json::from_str(frame).map_err(|error| format!("MCP frame: {error}; {body}"))
}

async fn tool(
    client: &reqwest::Client,
    url: &str,
    name: &str,
    arguments: Value,
) -> Result<Value, String> {
    mcp_call(
        client,
        url,
        "tools/call",
        json!({"name":name,"arguments":arguments}),
    )
    .await
}

#[tokio::test]
async fn human_mcp_prompt_and_exact_cancel_reach_real_host_runtime_and_broker() -> TestResult {
    // Arrange: the external ACP executable alone is controlled. All owning components are real.
    let root = tempfile::tempdir()?;
    let runtime = start_host(root.path()).await?;
    let mut readback = None;
    with_owned_host(runtime, async |_runtime| {
    let manifest: collaboration_protocol::ServiceManifest =
        serde_json::from_slice(&std::fs::read(root.path().join("service.json"))?)?;
    let url = manifest.mcp.url;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let initialized = mcp_call(
        &http,
        &url,
        "initialize",
        json!({
            "protocolVersion":"2025-11-25","capabilities":{},
            "clientInfo":{"name":"human-host-control","version":"1"}
        }),
    )
    .await?;
    check!(
        initialized["result"]["protocolVersion"].is_string(),
        "{initialized}"
    );
    let tools = mcp_call(&http, &url, "tools/list", json!({})).await?;
    for name in [
        "conversation_create",
        "conversation_prompt",
        "conversation_cancel",
    ] {
        check!(
            tools["result"]["tools"]
                .as_array()
                .ok_or("tool list")?
                .iter()
                .any(|tool| tool["name"] == name)
        );
    }
    let client = CollaborationClient::connect(root.path(), "human-control-readback", "1").await?;
    let inventory = client.list_endpoints().await?;
    let provider = inventory
        .endpoints
        .iter()
        .find(|entry| String::from(entry.endpoint.endpoint_id.clone()) == "claude-local")
        .ok_or("controlled endpoint missing")?;
    let generation = provider
        .channels
        .iter()
        .find_map(|channel| match channel {
            ChannelDescription::ExternalProvider {
                binding_generation, ..
            } => Some(CodexGeneration {
                service_epoch: inventory.service_epoch.clone(),
                generation: *binding_generation,
            }),
            _ => None,
        })
        .ok_or("binding generation missing")?;
    let human = json!({"kind":"human","humanId":"fixture-operator"});
    let approver = json!({"kind":"human","humanId":"independent-approver"});
    let create_id = OperationId::generate();
    let created = tool(
        &http,
        &url,
        "conversation_create",
        json!({
            "operationId":create_id,"endpoint":provider.endpoint,"generation":generation,
            "workingDirectory":root.path(),"access":"write-restricted",
            "createdBy":human,"approver":approver,"timeoutSeconds":5
        }),
    )
    .await?;
    check_eq!(created["result"]["isError"], false, "{created}");
    check_eq!(
        created["result"]["structuredContent"]["operationId"],
        json!(create_id)
    );
    let target: SessionRef =
        serde_json::from_value(created["result"]["structuredContent"]["target"].clone())?;
    let mut observer = HubEventFollow::new(&client, &target);
    let prompt_id = OperationId::generate();
    let prompt_http = http.clone();
    let prompt_url = url.clone();
    let arguments = json!({"operationId":prompt_id,"target":target,"generation":generation,
        "requestedBy":human,"approver":{"humanId":"independent-approver"},
        "message":{"kind":"humanUser","text":"request permission"},"timeoutSeconds":5});

    // Act: the peer holds the prompt at a real permission callback before cancellation.
    let prompt = tokio::spawn(async move {
        tool(&prompt_http, &prompt_url, "conversation_prompt", arguments).await
    });
    let (request_id, initial_events) = wait_for_pending(&client, &mut observer).await?;
    let approvals = client.list_approvals_with_options(true).await?;
    let approval = approvals
        .approvals
        .iter()
        .find(|approval| approval.request_id == request_id)
        .ok_or("pending approval missing")?;
    let approval = serde_json::to_value(approval)?;
    check_eq!(approval["requester"], json!(target));
    check_eq!(approval["approver"], approver);

    // The reported requester has no authority to decide the independent Approver's offer.
    let wrong_role = tool(
        &http,
        &url,
        "approval_decide",
        json!({
            "requestId":request_id,"actor":human,"optionId":"allow-once"
        }),
    )
    .await?;
    check_eq!(wrong_role["result"]["isError"], true, "{wrong_role}");
    check_eq!(
        wrong_role["result"]["structuredContent"]["serviceKind"],
        "wrongActor",
        "{wrong_role}"
    );
    check!(
        !prompt.is_finished(),
        "wrong-role decision settled the held prompt"
    );
    let cancel_id = OperationId::generate();
    let cancelled = tool(&http, &url, "conversation_cancel", json!({
        "operationId":cancel_id,"targetOperationId":prompt_id,"target":target,
        "generation":generation,"requestedBy":human,"approver":{"humanId":"independent-approver"}
    })).await?;
    check_eq!(cancelled["result"]["isError"], false, "{cancelled}");
    check_eq!(
        cancelled["result"]["structuredContent"]["operation"]["operationId"],
        json!(cancel_id)
    );
    let mut release = tokio::net::UnixStream::connect(root.path().join("exit.sock")).await?;
    use tokio::io::AsyncWriteExt as _;
    release.write_all(b"c").await?;
    let events = watch_turn(&mut observer, "turnEnded", initial_events).await?;
    assert_interaction_settles_before_turn_end(&events)?;
    let prompted = tokio::time::timeout(Duration::from_secs(5), prompt).await???;
    check_eq!(prompted["result"]["isError"], false, "{prompted}");
    check_eq!(
        prompted["result"]["structuredContent"]["target"],
        json!(target)
    );
    check_eq!(
        prompted["result"]["structuredContent"]["operationId"],
        json!(prompt_id)
    );
    check_eq!(
        prompted["result"]["structuredContent"]["settlement"]["stopReason"],
        "cancelled"
    );

    // Assert: real operation readback, binding generation, cancelled turn and durable roles.
    let cancellation = client
        .wait_for_provider_conversation_operation(ConversationOperationWaitRequest {
            operation_id: cancel_id.clone(),
            timeout_seconds: PositiveSeconds::try_from(3)?,
        })
        .await?;
    check!(
        matches!(cancellation.output, ConversationOperationWaitOutput::Available {
        settlement: ConversationOperationSettlement::CancelRequested { target: ref cancelled_target, target_operation_id: ref operation }
    } if cancelled_target == &target && operation == &prompt_id),
        "{cancellation:?}"
    );
    check_eq!(
        listed_state(&client, &target).await?,
        ProviderSessionState::Idle
    );
    drop(observer);
    readback = Some((target, generation, create_id, prompt_id, cancel_id));
    Ok(())
    }).await?;
    let (target, generation, create_id, prompt_id, cancel_id) =
        readback.ok_or("successful scenario omitted readback identities")?;
    let mut store = collaboration_runtime::ProviderOperationStore::open(
        &root.path().join("provider-operations.sqlite"),
    )
    .await?;
    let record = store
        .session_record(&target)
        .await?
        .ok_or("session record missing")?;
    check_eq!(
        serde_json::to_value(record.created_by)?,
        json!({"humanId":"fixture-operator"})
    );
    check_eq!(
        serde_json::to_value(record.approver)?,
        json!({"humanId":"independent-approver"})
    );
    for operation in [&create_id, &prompt_id, &cancel_id] {
        let record = store
            .inspect(operation)
            .await?
            .ok_or("durable operation missing")?;
        check_eq!(record.operation_id, *operation);
        check_eq!(record.target.as_ref(), Some(&target));
        check_eq!(
            record.stage,
            collaboration_protocol::ProviderOperationStage::Terminal
        );
        check_eq!(
            record.effect,
            collaboration_protocol::ProviderOperationEffect::Applied
        );
        if operation == &prompt_id {
            check_eq!(
                record.terminal_stop_reason,
                Some(collaboration_protocol::ProviderPromptStopReason::Cancelled)
            );
        }
        let collaboration_protocol::ConversationBindingIdentity::ExternalProvider { binding } =
            record.binding
        else {
            return Err("operation lost external binding".into());
        };
        check_eq!(binding.generation, generation);
    }
    Ok(())
}

#[tokio::test]
async fn dropped_human_mcp_waiter_does_not_cancel_or_redispatch_the_released_turn() -> TestResult {
    // Arrange: existing behavior peer with a test-owned release gate, through the real Host.
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let receipt = root.path().join("wire.jsonl");
    let release = root.path().join("release");
    let runtime = CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            owner_human_id: Some(message_board::HumanId::try_from(
                "fixture-operator".to_owned(),
            )?),
            directory: root.path().into(),
            codex_home: root.path().into(),
            backend_socket: root.path().join("backend.sock"),
            mcp_bind: ([127, 0, 0, 1], 0).into(),
            native_schema: None,
            peer_registry_directory: None,
            remote_control_server_name: None,
        },
        vec![ExternalProviderStartup::Launch(
            ExternalProviderLaunchBinding::cursor(
                node_test_fixture::node_executable()?,
                vec![
                    concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../acp-client-runtime/tests/support/behavior-agent.ts"
                    )
                    .into(),
                    receipt.display().to_string(),
                    "manual".into(),
                    "cursor_login".into(),
                    release.display().to_string(),
                ],
            )?,
        )],
    )
    .await?;
    with_owned_host(runtime, async |runtime| {
        let manifest: collaboration_protocol::ServiceManifest =
            serde_json::from_slice(&std::fs::read(root.path().join("service.json"))?)?;
        let url = manifest.mcp.url;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        let client =
            CollaborationClient::connect(root.path(), "detached-human-readback", "1").await?;
        let endpoint = json!({"serviceId":runtime.service_id(),"endpointId":"cursor-local"});
        let created = tool(&http,&url,"conversation_create",json!({
        "operationId":OperationId::generate(),"endpoint":endpoint,"workingDirectory":root.path(),
        "access":"write-restricted","createdBy":{"kind":"human","humanId":"fixture-operator"},
        "approver":{"kind":"human","humanId":"independent-approver"},"timeoutSeconds":5
    })).await?;
        check_eq!(created["result"]["isError"], false, "{created}");
        let target: SessionRef =
            serde_json::from_value(created["result"]["structuredContent"]["target"].clone())?;
        let mut observer = HubEventFollow::new(&client, &target);
        let prompt_id = OperationId::generate();
        let prompt_http = http.clone();
        let prompt_url = url.clone();
        let arguments = json!({"operationId":prompt_id,"target":target,
        "requestedBy":{"kind":"human","humanId":"fixture-operator"},
        "approver":{"humanId":"independent-approver"},
        "message":{"kind":"humanUser","text":"hold"},"timeoutSeconds":5});
        let prompt = tokio::spawn(async move {
            tool(&prompt_http, &prompt_url, "conversation_prompt", arguments).await
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = observer.next_message().await?;
                if event.pointer("/event/item/text") == Some(&json!("HOLDING")) {
                    return Ok::<(), Box<dyn std::error::Error>>(());
                }
            }
        })
        .await??;

        // Act: first destroy the HTTP waiter, then release that already-admitted work.
        check!(!release.exists());
        prompt.abort();
        let aborted = prompt.await;
        check!(
            matches!(aborted, Err(ref error) if error.is_cancelled()),
            "HTTP waiter was not aborted: {aborted:?}"
        );
        std::fs::write(&release, b"release the held turn")?;
        let settled = client
            .wait_for_provider_conversation_operation(ConversationOperationWaitRequest {
                operation_id: prompt_id.clone(),
                timeout_seconds: PositiveSeconds::try_from(3)?,
            })
            .await;
        let settled = match settled {
            Ok(settled) => settled,
            Err(error) => {
                eprintln!(
                    "controlled peer receipt: {}",
                    std::fs::read_to_string(&receipt)?
                );
                drop(observer);
                return Err(error.into());
            }
        };

        // Assert: real provider response and one actual prompt, with no actual cancellation.
        let ConversationOperationWaitOutput::Available {
            settlement:
                ConversationOperationSettlement::PromptCompleted {
                    target: ref returned,
                    stop_reason: collaboration_protocol::ProviderPromptStopReason::EndTurn,
                    response: Some(ref response),
                },
        } = settled.output
        else {
            // Settle the owned peer even on the old fixture's expected RED.
            eprintln!(
                "controlled peer receipt: {:?}",
                std::fs::read_to_string(&receipt)
            );
            drop(observer);
            return Err(
                format!("released detached operation did not complete: {settled:?}").into(),
            );
        };
        check_eq!(returned, &target);
        check_eq!(response.as_str(), "HOLDINGafter");
        let rows = std::fs::read_to_string(&receipt)?
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>()?;
        let received = |method: &str| {
            rows.iter()
                .filter(|row| {
                    row["method"] == method
                        && row["params"]["sessionId"] == json!(target.session_id)
                })
                .count()
        };
        check_eq!(received("session/prompt"), 1, "{rows:?}");
        check_eq!(received("session/cancel"), 0, "{rows:?}");
        check_eq!(
            rows.iter()
                .filter(
                    |row| row["kind"] == "release" && row["sessionId"] == json!(target.session_id)
                )
                .count(),
            1,
            "{rows:?}"
        );
        drop(observer);
        Ok(())
    })
    .await
}

#[tokio::test]
async fn failed_held_observation_still_closes_the_owned_provider() -> TestResult {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    const FAILURE: &str = "deliberate observation failure after the real permission callback";
    let root = tempfile::tempdir()?;
    let runtime = start_host(root.path()).await?;
    let socket = root.path().join("exit.sock");
    let mut witness = None;
    let observed = with_owned_host(runtime, async |_runtime| {
        let client = CollaborationClient::connect(root.path(), "failed-observation", "1").await?;
        let (target, actor, approver, generation) = create_session(&client, root.path()).await?;
        let mut observer = HubEventFollow::new(&client, &target);
        let (_operation, _prompt) = prompt(&client, &target, &actor, &approver, &generation)?;
        let _pending = wait_for_pending(&client, &mut observer).await?;
        // Retain a genuine peer connection and the temp root across finalization.
        witness = Some(tokio::net::UnixStream::connect(&socket).await?);
        Err(FAILURE.into())
    })
    .await;
    check_eq!(
        observed.as_ref().err().map(ToString::to_string),
        Some(FAILURE.to_owned())
    );
    let mut witness = witness.ok_or("the failed observation never reached its peer")?;
    let mut byte = [0_u8; 1];
    let read = tokio::time::timeout(Duration::from_secs(5), witness.read(&mut byte)).await;
    if !matches!(read, Ok(Ok(0))) {
        // Preserve the failure and settle this test-owned peer even on a deliberately bad runner.
        let stop = async {
            let mut peer = tokio::net::UnixStream::connect(&socket).await?;
            peer.write_all(b"x").await?;
            Ok::<(), std::io::Error>(())
        }
        .await;
        let stopped_eof =
            tokio::time::timeout(Duration::from_secs(5), witness.read(&mut byte)).await;
        return Err(format!("owned peer did not reach EOF after failed-observation cleanup: {read:?}; fixture stop: {stop:?}; stopped EOF: {stopped_eof:?}").into());
    }
    match tokio::net::UnixStream::connect(&socket).await {
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => Ok(()),
        Err(error) => Err(error.into()),
        Ok(mut leaked) => {
            // Keep a failing sensitivity run from creating another orphan.
            use tokio::io::AsyncWriteExt as _;
            leaked.write_all(b"x").await?;
            Err("failed observation left its controlled peer accepting connections".into())
        }
    }
}
