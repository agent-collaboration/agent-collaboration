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
    ConversationResumeRequest, EndpointId, EndpointRef, GenerationNumber, MessageContent,
    MessageText, NonEmptyText, OperationId, PositiveSeconds, ProviderBindingId,
    ProviderBindingIdentity, ProviderCapabilities, ProviderCapability, ProviderCapabilityEvidence,
    ProviderCapabilityName, ProviderCapabilityStatus, ProviderKind, ProviderOperationEffect,
    ProviderOperationStage, ProviderPromptStopReason, ProviderReconciliationState,
    ProviderRequestedPolicy, ProviderRequestedSettings, ProviderRuntimeIdentity,
    ProviderSessionInspectRequest, ProviderSettingName, ProviderSettingsAcceptRequest,
    ProviderSettingsFailureKind, ProviderSettingsSetRequest, ProviderTransport,
    ProviderWorkingDirectory, RouterAccess, SessionId, SessionRef, UuidIdentity,
};
use collaboration_runtime::ProviderOperationStore;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

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

async fn wait_for_fixture_loads(admission_log: &std::path::Path, expected: usize) {
    loop {
        let received = tokio::fs::read_to_string(admission_log)
            .await
            .unwrap_or_default()
            .lines()
            .count();
        if received >= expected {
            assert_eq!(received, expected, "each restore should dispatch once");
            return;
        }
        tokio::task::yield_now().await;
    }
}

fn assert_process_reaped(process_id_path: &std::path::Path) -> TestResult {
    let process_id = std::fs::read_to_string(process_id_path)?.parse::<i32>()?;
    let process_id = rustix::process::Pid::from_raw(process_id).ok_or("invalid provider PID")?;
    if !matches!(
        rustix::process::test_kill_process(process_id),
        Err(rustix::io::Errno::SRCH)
    ) {
        return Err("provider process still exists after shutdown joined its runtime".into());
    }
    Ok(())
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

#[path = "../external_provider_supervisor/settings_projection_tests.rs"]
mod settings_projection_tests;

#[path = "../external_provider_supervisor/lifecycle_tests.rs"]
mod lifecycle_tests;

#[path = "../external_provider_supervisor/supervisor_shutdown_tests.rs"]
mod supervisor_shutdown_tests;

#[path = "../external_provider_supervisor/initialization_authentication_test.rs"]
mod initialization_authentication_tests;

#[path = "../external_provider_supervisor/cursor_settings_tests.rs"]
mod cursor_settings_tests;

#[path = "../external_provider_supervisor/start_mode_tests.rs"]
mod start_mode_tests;
