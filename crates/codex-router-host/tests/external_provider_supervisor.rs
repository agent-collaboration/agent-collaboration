use codex_router_host::{
    ExternalProviderBinding, ExternalProviderLaunch, ExternalProviderRuntime,
    ExternalProviderSupervisor,
};
use collaboration_contracts::ProviderConversationBackend;
use collaboration_protocol::{
    CodexGeneration, ConversationAdmissionState, ConversationCancelRequest,
    ConversationCloseRequest, ConversationCreateRequest, ConversationLoadRequest,
    ConversationOperationFailureKind, ConversationOperationReconcileRequest,
    ConversationOperationSettlement, ConversationOperationShowRequest,
    ConversationOperationWaitOutput, ConversationOperationWaitRequest, ConversationPromptRequest,
    ConversationResumeRequest, EndpointDescription, EndpointId, EndpointRef, GenerationNumber,
    MessageContent, MessageText, NonEmptyText, OperationId, PositiveSeconds, ProviderBindingId,
    ProviderBindingIdentity, ProviderCapabilities, ProviderCapability, ProviderCapabilityEvidence,
    ProviderCapabilityName, ProviderCapabilityStatus, ProviderKind, ProviderOperationEffect,
    ProviderOperationStage, ProviderPromptStopReason, ProviderReconciliationState,
    ProviderRequestedPolicy, ProviderRequestedSettings, ProviderRuntimeIdentity,
    ProviderSessionInspectRequest, ProviderSettingName, ProviderSettingsAcceptRequest,
    ProviderSettingsFailureKind, ProviderSettingsSetRequest, ProviderTransport,
    ProviderWorkingDirectory, RouterAccess, SessionId, SessionRef, UuidIdentity,
};
use collaboration_runtime::EndpointDirectory;
use collaboration_runtime::NativeControlBackend;
use collaboration_runtime::NativeGenerationGate;
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ServiceInteractionBroker;
use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

#[path = "../src/external_provider_runtime/approval_push_fixture.rs"]
mod approval_push_fixture;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

macro_rules! ensure {
    ($condition:expr) => {
        if !$condition {
            return Err(format!("assertion failed: {}", stringify!($condition)).into());
        }
    };
}

macro_rules! ensure_eq {
    ($left:expr, $right:expr) => {{
        let left = &$left;
        let right = &$right;
        if left != right {
            return Err(format!(
                "assertion failed: {} == {}: left={left:?}, right={right:?}",
                stringify!($left),
                stringify!($right)
            )
            .into());
        }
    }};
}

fn endpoint(name: &str) -> TestResult<EndpointRef> {
    Ok(EndpointRef {
        service_id: UuidIdentity::try_from("0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89".to_owned())?,
        endpoint_id: EndpointId::try_from(name.to_owned())?,
    })
}

fn generation() -> TestResult<CodexGeneration> {
    Ok(CodexGeneration {
        service_epoch: UuidIdentity::try_from("1ff962c5-7fa3-4c18-a5ca-1bbe8db09e80".to_owned())?,
        generation: GenerationNumber::try_from(1)?,
    })
}

fn binding(endpoint: EndpointRef, name: &str) -> TestResult<ProviderBindingIdentity> {
    Ok(ProviderBindingIdentity {
        endpoint,
        binding_id: ProviderBindingId::try_from(format!("{name}-binding"))?,
        runtime: ProviderRuntimeIdentity {
            provider: ProviderKind::ClaudeCode,
            runtime_name: NonEmptyText::try_from(name.to_owned())?,
            runtime_version: Some(NonEmptyText::try_from("1".to_owned())?),
        },
        transport: ProviderTransport::StdioAcp,
        generation: generation()?,
        capabilities: ProviderCapabilities::try_from(vec![
            ProviderCapability {
                name: ProviderCapabilityName::Create,
                status: ProviderCapabilityStatus::Supported,
                evidence: ProviderCapabilityEvidence::Advertised,
            },
            ProviderCapability {
                name: ProviderCapabilityName::Prompt,
                status: ProviderCapabilityStatus::Supported,
                evidence: ProviderCapabilityEvidence::Advertised,
            },
            ProviderCapability {
                name: ProviderCapabilityName::Cancel,
                status: ProviderCapabilityStatus::Supported,
                evidence: ProviderCapabilityEvidence::Advertised,
            },
        ])?,
    })
}

fn actor(endpoint: EndpointRef, id: &str) -> TestResult<SessionRef> {
    Ok(SessionRef {
        endpoint,
        session_id: SessionId::try_from(id.to_owned())?,
    })
}

fn policy() -> ProviderRequestedPolicy {
    ProviderRequestedPolicy {
        access: RouterAccess::WriteRestricted,
    }
}

fn working_directory() -> TestResult<ProviderWorkingDirectory> {
    Ok(ProviderWorkingDirectory::try_from("/tmp".to_owned())?)
}

fn launch(program: &str, arguments: Vec<String>) -> ExternalProviderLaunch {
    let script = node_test_fixture::fixture_program(program);
    ExternalProviderLaunch {
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        executable: PathBuf::from("node"),
        arguments: std::iter::once(script.to_string_lossy().into_owned())
            .chain(arguments)
            .collect(),
        environment: vec![],
    }
}

fn cancel_fixture(dispatch_log: &std::path::Path) -> ExternalProviderLaunch {
    launch(
        "supervisor-integration-agent.ts",
        vec![
            "cancel".to_owned(),
            dispatch_log.to_string_lossy().into_owned(),
        ],
    )
}

fn load_fixture() -> ExternalProviderLaunch {
    launch("supervisor-integration-agent.ts", vec!["load".to_owned()])
}

fn resume_close_fixture() -> ExternalProviderLaunch {
    launch(
        "supervisor-integration-agent.ts",
        vec!["lifecycle".to_owned()],
    )
}

fn permission_allow_fixture() -> ExternalProviderLaunch {
    launch(
        "supervisor-integration-agent.ts",
        vec!["permission".to_owned()],
    )
}

fn permission_cancel_fixture() -> ExternalProviderLaunch {
    launch(
        "supervisor-integration-agent.ts",
        vec!["retirement".to_owned()],
    )
}

fn authentication_then_create_fixture() -> ExternalProviderLaunch {
    launch(
        "supervisor-integration-agent.ts",
        vec!["authentication".to_owned()],
    )
}

fn authentication_then_prompt_fixture() -> ExternalProviderLaunch {
    launch(
        "supervisor-integration-agent.ts",
        vec!["prompt-authentication".to_owned()],
    )
}

fn provider_prompt_rejection_fixture() -> ExternalProviderLaunch {
    launch(
        "supervisor-integration-agent.ts",
        vec!["rejection".to_owned()],
    )
}

fn saturated_load_fixture(
    admission_log: &std::path::Path,
    process_id_path: &std::path::Path,
) -> ExternalProviderLaunch {
    launch(
        "host-admission-agent.ts",
        vec![
            "supervisor-saturated".to_owned(),
            admission_log.to_string_lossy().into_owned(),
            process_id_path.to_string_lossy().into_owned(),
        ],
    )
}

async fn assert_process_reaped(process_id_path: &std::path::Path) -> TestResult {
    let process_id = std::fs::read_to_string(process_id_path)?.parse::<i32>()?;
    let process_id = rustix::process::Pid::from_raw(process_id).ok_or("invalid provider PID")?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if matches!(
                rustix::process::test_kill_process(process_id),
                Err(rustix::io::Errno::SRCH)
            ) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

async fn approval_broker_fixture(
    root: &tempfile::TempDir,
    service_id: &UuidIdentity,
    approver: &SessionRef,
) -> TestResult<(
    Arc<ServiceInteractionBroker>,
    tokio::task::JoinHandle<Result<Vec<Value>, Box<dyn std::error::Error + Send + Sync>>>,
)> {
    let socket_path = root.path().join("native-approver.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path)?;
    let native_generation = generation()?;
    let mut definitions = serde_json::Map::new();
    for operation in [
        "ThreadRead",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
        "ThreadQueueAdd",
    ] {
        definitions.insert(format!("{operation}Params"), json!({"type":"object"}));
        definitions.insert(format!("{operation}Response"), json!({"type":"object"}));
    }
    let bundle = codex_native_integration::NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))?,
    )]))?;
    let schemas = Arc::new(codex_native_integration::NativePayloadSchemas::from_bundle(
        &bundle,
    )?);
    let gate = NativeGenerationGate::default();
    gate.activate(
        native_generation.clone(),
        socket_path,
        Some(schemas.clone()),
    )?;
    let endpoint_description: EndpointDescription = serde_json::from_value(json!({
        "endpoint": approver.endpoint,
        "label": "Fixture approver",
        "availability": {"state":"available","observedAt":"2026-09-20T00:00:00Z"},
        "channels": [{
            "kind":"nativeCodex",
            "transport":"unixWebSocket",
            "path":"native-approver.sock",
            "schemaDigest":schemas.schema_digest(),
            "generation":native_generation
        }]
    }))?;
    let endpoints = EndpointDirectory::new(service_id.clone());
    endpoints.publish(endpoint_description)?;
    let native_backend = NativeControlBackend {
        codex_home: root.path().to_path_buf(),
        endpoint: approver.endpoint.clone(),
        gate,
    };
    let interaction_store = Arc::new(
        interaction_storage::InteractionStore::open(root.path().join("interaction.sqlite")).await?,
    );
    let broker = ServiceInteractionBroker::load(
        service_id.clone(),
        native_backend.clone(),
        interaction_store,
    )
    .await?;
    let route: Arc<dyn collaboration_contracts::SessionDeliveryRoute> =
        Arc::new(collaboration_runtime::CodexAppServerDeliveryRoute::new(
            service_id.clone(),
            endpoints,
            native_backend,
            Arc::new(collaboration_runtime::UnmaterializedThreadHolder::new()),
        ));
    broker.install_session_delivery(Arc::new(
        collaboration_runtime::SessionDeliveryRouter::new(vec![route]),
    ))?;
    let push_fixture =
        approval_push_fixture::ApprovalPushFixture::compose(root.path(), service_id, &broker)
            .await
            .map_err(|error| format!("approval push fixture composition: {error}"))?;
    let captured_approver = approver.clone();
    let backend = tokio::spawn(async move {
        let (stream, _) =
            tokio::time::timeout(std::time::Duration::from_secs(2), listener.accept())
                .await
                .map_err(|_| "native approval push did not connect")??;
        let mut socket = tokio_tungstenite::accept_async(stream).await?;
        let mut requests = Vec::new();
        for expected_method in ["initialize", "initialized", "thread/read", "turn/start"] {
            let frame = tokio::time::timeout(std::time::Duration::from_secs(2), socket.next())
                .await
                .map_err(|error| {
                    std::io::Error::other(format!(
                        "native approver timed out waiting for {expected_method}: {error}"
                    ))
                })?
                .ok_or_else(|| {
                    std::io::Error::other(format!(
                        "native approver connection closed while waiting for {expected_method}"
                    ))
                })?
                .map_err(|error| {
                    std::io::Error::other(format!(
                        "native approver websocket failed while waiting for {expected_method}: {error}"
                    ))
                })?;
            let request: Value = serde_json::from_str(frame.to_text()?)?;
            ensure_eq!(
                request.get("method").and_then(Value::as_str),
                Some(expected_method)
            );
            if expected_method == "turn/start" {
                let delivered_line = request
                    .pointer("/params/input/0/text")
                    .and_then(Value::as_str)
                    .ok_or("native approver input omitted its push line")?;
                let _record = push_fixture
                    .captured_approval(delivered_line, &captured_approver)
                    .await?;
            }
            requests.push(request.clone());
            if expected_method == "initialized" {
                continue;
            }
            let result = match expected_method {
                "thread/read" => json!({"thread":{"id":"approver","status":{"type":"idle"}}}),
                "turn/start" => json!({"turn":{"id":"approval-turn"}}),
                _ => json!({}),
            };
            socket
                .send(Message::Text(
                    json!({"id":request.get("id").ok_or("native request ID missing")?,"result":result})
                        .to_string()
                        .into(),
                ))
                .await?;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(requests)
    });
    Ok((broker, backend))
}

async fn supervisor(
    root: &tempfile::TempDir,
    bindings: Vec<(ProviderBindingIdentity, ExternalProviderRuntime)>,
) -> TestResult<ExternalProviderSupervisor> {
    let store =
        ProviderOperationStore::open(&root.path().join("provider-operations.sqlite")).await?;
    Ok(ExternalProviderSupervisor::new(
        bindings
            .into_iter()
            .map(|(identity, runtime)| ExternalProviderBinding { identity, runtime })
            .collect(),
        Arc::new(Mutex::new(store)),
    )?)
}

#[expect(
    clippy::expect_used,
    clippy::result_large_err,
    reason = "A malformed fixed fixture must fail its test."
)]
async fn wait(
    backend: &dyn ProviderConversationBackend,
    operation_id: OperationId,
) -> Result<
    collaboration_protocol::ConversationOperationWaitResult,
    collaboration_protocol::ConversationOperationFailure,
> {
    backend
        .wait(ConversationOperationWaitRequest {
            operation_id,
            timeout_seconds: PositiveSeconds::try_from(2).expect("positive timeout"),
        })
        .await
}

fn operation<T>(
    result: Result<T, collaboration_protocol::ConversationOperationFailure>,
) -> TestResult<T> {
    result.map_err(|error| format!("provider operation failed: {error:?}").into())
}

async fn await_admission(
    backend: &dyn ProviderConversationBackend,
    operation_id: &OperationId,
) -> TestResult {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            match backend
                .show(ConversationOperationShowRequest {
                    operation_id: operation_id.clone(),
                })
                .await
            {
                Ok(_) => return Ok(()),
                Err(error) if error.kind == ConversationOperationFailureKind::NotFound => {
                    tokio::task::yield_now().await;
                }
                Err(error) => {
                    return Err(format!("provider admission failed: {error:?}").into());
                }
            }
        }
    })
    .await
    .map_err(|_| "provider admission timed out")?
}

#[path = "external_provider_supervisor/settings_projection_tests.rs"]
mod settings_projection_tests;

#[path = "external_provider_supervisor/lifecycle_tests.rs"]
mod lifecycle_tests;

#[path = "external_provider_supervisor/cursor_settings_tests.rs"]
mod cursor_settings_tests;
