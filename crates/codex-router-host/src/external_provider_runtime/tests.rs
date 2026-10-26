use super::*;

#[cfg(unix)]
fn node_fixture(program: &str, arguments: Vec<String>) -> ExternalProviderLaunch {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../verification/fixtures")
        .join(program);
    ExternalProviderLaunch {
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        executable: PathBuf::from("node"),
        arguments: std::iter::once(script.to_string_lossy().into_owned())
            .chain(arguments)
            .collect(),
        environment: vec![],
    }
}

#[test]
fn provider_initialize_errors_expose_only_safe_metadata() {
    let account_sentinel = "synthetic-account-sentinel@example.invalid";
    let token_sentinel = "synthetic-token-sentinel-7f4e";
    let error = agent_client_protocol::Error::new(-32001, format!("denied {token_sentinel}"))
        .data(serde_json::json!({"account": account_sentinel, "token": token_sentinel}));

    let diagnostic = sanitized_initialization_error(&error);

    assert!(diagnostic.contains("initialize"));
    assert!(diagnostic.contains("stage=initialize"));
    assert!(diagnostic.contains("-32001"));
    assert!(diagnostic.contains("error_data_bytes="));
    assert!(!diagnostic.contains(account_sentinel));
    assert!(!diagnostic.contains(token_sentinel));

    let operation_error = sanitized_acp_error(&error, "session/update", "load replay");
    assert!(operation_error.contains("method=session/update"));
    assert!(operation_error.contains("stage=load replay"));
    assert!(operation_error.contains("-32001"));
    assert!(!operation_error.contains(account_sentinel));
    assert!(!operation_error.contains(token_sentinel));
}

#[cfg(unix)]
fn initialization_fixture(
    response_version: u16,
    process_id_path: Option<&std::path::Path>,
) -> ExternalProviderLaunch {
    node_fixture(
        "host-initialization-agent.ts",
        vec![
            response_version.to_string(),
            process_id_path
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ],
    )
}

#[cfg(unix)]
async fn assert_process_reaped(process_id_path: &std::path::Path) {
    let process_id = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(value) = std::fs::read_to_string(process_id_path)
                && let Ok(process_id) = value.trim().parse::<i32>()
            {
                break process_id;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("fixture wrote process id");
    let process_id = rustix::process::Pid::from_raw(process_id).expect("positive process id");
    assert!(matches!(
        rustix::process::test_kill_process(process_id),
        Err(rustix::io::Errno::SRCH)
    ));
}

#[cfg(unix)]
fn conversation_fixture() -> ExternalProviderLaunch {
    node_fixture(
        "host-basic-session-agent.ts",
        vec!["conversation".to_owned()],
    )
}

#[cfg(unix)]
fn steering_fixture(observed_prompt_socket: &std::path::Path) -> ExternalProviderLaunch {
    node_fixture(
        "host-steering-agent.ts",
        vec![observed_prompt_socket.to_string_lossy().into_owned()],
    )
}

#[cfg(unix)]
#[tokio::test]
async fn steering_injects_during_prompt_and_returns_prompt_required_when_idle() {
    let root = tempfile::tempdir().expect("fixture root");
    let marker = root.path().join("prompt.sock");
    let listener = tokio::net::UnixListener::bind(&marker).expect("prompt event listener");
    let runtime = std::sync::Arc::new(
        ExternalProviderRuntime::initialize(steering_fixture(&marker))
            .await
            .expect("provider runtime"),
    );
    assert!(runtime.admission().supports_steering);
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session/new");
    let running_operation_id =
        OperationId::try_from("018f1f62-6571-7ef0-8f0c-001122334499".to_owned())
            .expect("running operation ID");
    let prompt_runtime = std::sync::Arc::clone(&runtime);
    let prompt_operation_id = running_operation_id.clone();
    let (dispatch, dispatched) = tokio::sync::oneshot::channel();
    let prompt = tokio::spawn(async move {
        prompt_runtime
            .prompt_for_operation(
                "fixture-session".to_owned(),
                Some(prompt_operation_id),
                "first".to_owned(),
                Some(dispatch),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), dispatched)
        .await
        .expect("prompt dispatch notification")
        .expect("prompt was sent before settlement");
    // Read the notice to EOF instead of dropping the accepted stream: an early
    // close makes the fixture's `sendall` fail with EPIPE, which kills the
    // provider and surfaces as a spurious steering TransportFailure.
    let prompt_notice = tokio::time::timeout(Duration::from_secs(2), async {
        let (mut stream, _address) = listener.accept().await.expect("prompt notification");
        let mut notice = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut notice)
            .await
            .expect("prompt notice payload");
        notice
    })
    .await
    .expect("prompt observed before steer");
    assert_eq!(prompt_notice, b"prompt");
    assert_eq!(
        runtime
            .session_activity("fixture-session".to_owned())
            .await
            .expect("active session state"),
        ProviderSessionActivity::Running
    );
    let wait_runtime = std::sync::Arc::clone(&runtime);
    let waiting = tokio::spawn(async move {
        wait_runtime
            .wait_session_idle("fixture-session".to_owned())
            .await
    });

    let injected = runtime
        .steer_session("fixture-session".to_owned(), "follow-up".to_owned())
        .await
        .expect("steer active turn");
    assert_eq!(
        injected,
        ProviderSteeringOutcome::Injected {
            running_operation_id: Some(running_operation_id),
        }
    );
    prompt
        .await
        .expect("prompt task")
        .expect("prompt settlement");
    waiting.await.expect("idle wait task").expect("idle event");
    assert_eq!(
        runtime
            .session_activity("fixture-session".to_owned())
            .await
            .expect("idle state"),
        ProviderSessionActivity::Idle
    );
    let idle = runtime
        .steer_session("fixture-session".to_owned(), "later".to_owned())
        .await
        .expect("steer idle session");
    assert_eq!(idle, ProviderSteeringOutcome::PromptRequired);
    runtime.shutdown().await;
}

#[cfg(unix)]
fn cancellation_fixture() -> ExternalProviderLaunch {
    node_fixture("host-basic-session-agent.ts", vec!["cancel".to_owned()])
}

#[cfg(unix)]
fn successor_prompt_fixture() -> ExternalProviderLaunch {
    node_fixture("host-basic-session-agent.ts", vec!["successor".to_owned()])
}

#[cfg(unix)]
fn concurrent_admission_cancel_fixture(
    admission_method: &str,
    admission_observed: &std::path::Path,
) -> ExternalProviderLaunch {
    node_fixture(
        "host-admission-agent.ts",
        vec![
            "concurrent".to_owned(),
            admission_method.to_owned(),
            admission_observed.to_string_lossy().into_owned(),
        ],
    )
}

#[cfg(unix)]
fn saturated_load_admission_fixture(
    admission_log: &std::path::Path,
    process_id_path: &std::path::Path,
) -> ExternalProviderLaunch {
    node_fixture(
        "host-admission-agent.ts",
        vec![
            "saturated".to_owned(),
            admission_log.to_string_lossy().into_owned(),
            process_id_path.to_string_lossy().into_owned(),
        ],
    )
}

#[cfg(unix)]
fn permission_request_fixture() -> ExternalProviderLaunch {
    node_fixture("host-basic-session-agent.ts", vec!["permission".to_owned()])
}

#[cfg(unix)]
fn named_mcp_tool_call_fixture(
    raw_output: Option<serde_json::Value>,
    response_text: &str,
    split_result_before_status: bool,
) -> ExternalProviderLaunch {
    node_fixture(
        "host-tool-output-agent.ts",
        vec![
            serde_json::to_string(&raw_output).expect("MCP result JSON"),
            response_text.to_owned(),
            split_result_before_status.to_string(),
        ],
    )
}

fn approval_context(operation_id: &str) -> ExternalProviderApprovalContext {
    let service_id = "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89";
    ExternalProviderApprovalContext {
        requester: serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":service_id,"endpointId":"cursor-local"},"sessionId":"requester"})).expect("requester"),
        approver: serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":service_id,"endpointId":"codex-local"},"sessionId":"approver"})).expect("approver"),
        target: serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":service_id,"endpointId":"cursor-local"},"sessionId":"fixture-session"})).expect("target"),
        operation_id: operation_id.to_owned().try_into().expect("operation ID"),
        binding_generation: serde_json::from_value(serde_json::json!({"serviceEpoch":service_id,"generation":1})).expect("generation"),
        binding_retirement: CancellationToken::new(),
    }
}

#[test]
fn acp_error_codes_classify_authentication_without_message_matching() {
    let mut authentication = agent_client_protocol::Error::auth_required();
    authentication.message = "provider conversation is busy".to_owned();
    assert!(matches!(
        acp_operation_error(authentication),
        ExternalProviderRuntimeError::AuthenticationRequired { code: -32000, .. }
    ));
    for message in [
        "provider conversation is busy",
        "has not been created or loaded",
        "has no active prompt",
        "is no longer active",
    ] {
        let mut provider = agent_client_protocol::Error::internal_error();
        provider.message = message.to_owned();
        provider.data = Some(serde_json::json!({"privateText":"must not escape"}));
        let reason = acp_operation_error(provider);
        assert!(matches!(
            &reason,
            ExternalProviderRuntimeError::ProviderRejected { code, .. } if *code == -32603
        ));
        assert!(!reason.to_string().contains("must not escape"));
        assert!(!reason.to_string().contains(message));
    }

    let mut missing = agent_client_protocol::Error::new(-32002, "private missing-session text");
    missing.data = Some(serde_json::json!({"privateText":"must not escape"}));
    let reason = acp_operation_error(missing);
    assert!(matches!(
        &reason,
        ExternalProviderRuntimeError::ResourceNotFound { code, .. } if *code == -32002
    ));
    assert!(!reason.to_string().contains("private"));
}

#[test]
fn acp_error_code_table_uses_typed_safe_outcomes() {
    // ACP v1 error-codes.mdx maps JSON-RPC codes independently of agent text.
    // Specification R7 specializes -32002 only for session/load.
    for (code, expected_diagnostic) in [
        (
            -32000,
            "provider authentication is required (ACP code -32000)",
        ),
        (-32002, "provider resource was not found (ACP code -32002)"),
        (
            -32601,
            "provider ACP method is unsupported (ACP code -32601)",
        ),
        (
            -32602,
            "provider ACP parameters are invalid (ACP code -32602)",
        ),
        (
            -32800,
            "provider ACP request was cancelled (ACP code -32800)",
        ),
        (
            -32603,
            "provider rejected the ACP operation (ACP code -32603)",
        ),
    ] {
        let mut provider = agent_client_protocol::Error::new(code, "private provider text");
        provider.data = Some(serde_json::json!({"privateText": "secret sentinel"}));
        let diagnostic = acp_operation_error(provider).to_string();
        assert_eq!(diagnostic, expected_diagnostic, "code {code}");
        assert!(!diagnostic.contains("private"));
        assert!(!diagnostic.contains("secret sentinel"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn resource_not_found_from_session_load_is_session_not_found() {
    // ACP v1 error-codes.mdx: -32002 is a missing resource. R7 specializes
    // it only where the method contract names the resource as the Session.
    let fixture = acp_scripted_fixture::AcpFixtureScript::new()
        .expect_request("initialize", "initialize", serde_json::json!({"protocolVersion": 1}))
        .respond("initialize", serde_json::json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": true}, "agentInfo": {"name": "missing-load-fixture", "version": "1"}}))
        .expect_request("load", "session/load", serde_json::json!({"sessionId": "missing-session"}))
        .respond_error("load", -32002)
        .launch();
    let runtime = ExternalProviderRuntime::initialize(fixture)
        .await
        .expect("fixture initializes");
    let error = runtime
        .load_session("missing-session".to_owned(), PathBuf::from("/tmp"))
        .await
        .expect_err("missing session");
    assert!(matches!(
        error,
        ExternalProviderRuntimeError::ProviderSessionNotFound { code: -32002, .. }
    ));
    runtime.shutdown().await;
}

#[cfg(unix)]
fn aggregate_output_fixture() -> ExternalProviderLaunch {
    node_fixture("host-basic-session-agent.ts", vec!["aggregate".to_owned()])
}

#[cfg(unix)]
fn load_replay_fixture() -> ExternalProviderLaunch {
    node_fixture("host-basic-session-agent.ts", vec!["replay".to_owned()])
}

#[cfg(unix)]
fn mcp_session_setup_fixture(method: &str) -> ExternalProviderLaunch {
    node_fixture(
        "host-basic-session-agent.ts",
        vec!["mcp".to_owned(), method.to_owned()],
    )
}

#[path = "tests/admission_lifecycle_tests.rs"]
mod admission_lifecycle_tests;

#[path = "tests/mcp_shutdown_tests.rs"]
mod mcp_shutdown_tests;
