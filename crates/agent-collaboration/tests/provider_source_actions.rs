//! The public fixed-program source entry must load from actual Stored metadata.
//! An owned Host/storage/API are real; only its external ACP executable is supplied.
use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{
    OperationId, ProviderIdentity, ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess,
    SessionRef,
};
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionRecord;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt as _, time::Duration};

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "integration/configured_inventory_read/recorded_api_listener.rs"]
mod recorded_api_listener;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn public_source_load_and_resume_use_exact_stored_policy_directory_and_approver() {
    let result:TestResult=async {
    for provider_kind in [
        collaboration_protocol::ProviderKind::Cursor,
        collaboration_protocol::ProviderKind::ClaudeCode,
    ] {
        let endpoint_id = match provider_kind {
            collaboration_protocol::ProviderKind::Cursor => "cursor-local",
            collaboration_protocol::ProviderKind::ClaudeCode => "claude-local",
        };
        let root = tempfile::Builder::new()
            .prefix("source-action-")
            .tempdir_in("/tmp")?;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
        let root_path = std::fs::canonicalize(root.path())?;
        let provider = root_path.join("cursor-fixture.ts");
        let provider_log = root_path.join("provider-input.jsonl");
        codex_router_test_support::node_fixture::write_node_fixture(&provider, "source-policy-agent.ts", &json!(null))?;
        std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o700))?;
        let saved_directory = root_path.join("saved-restricted-project");
        let invoking_directory = root_path.join("different-new-default-project");
        std::fs::create_dir(&saved_directory)?;
        std::fs::create_dir(&invoking_directory)?;
        let runtime = Box::pin(CollaborationRuntime::start_with_external_providers(
            CollaborationRuntimeInputs {
                board_session: None,
                directory: root_path.clone(),
                codex_home: root_path.clone(),
                backend_socket: root_path.join("unused.sock"),
                mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
                native_schema: None,
                peer_registry_directory: None,
                remote_control_server_name: None,
                owner_human_id: None,
            },
            vec![ExternalProviderStartup::Launch(match provider_kind {
                collaboration_protocol::ProviderKind::Cursor => {
                    ExternalProviderLaunchBinding::cursor(
                        provider,
                        vec![provider_log.display().to_string()],
                    )?
                }
                collaboration_protocol::ProviderKind::ClaudeCode => {
                    let mut binding = ExternalProviderLaunchBinding::claude(
                        provider,
                        vec![provider_log.display().to_string()],
                    )?;
                    codex_router_test_support::global_claude_fixture::declare_global_claude_fixture(
                            &root_path,
                            &mut binding.launch.environment,
                        )?;
                    binding
                }
            })],
        ))
        .await?;
        let client =
            CollaborationClient::connect(&root_path, "source-policy-observer", "1").await?;
        let endpoint = client
            .list_endpoints()
            .await?
            .endpoints
            .into_iter()
            .find(|entry| String::from(entry.endpoint.endpoint_id.clone()) == endpoint_id)
            .ok_or("Cursor fixture endpoint missing")?;
        let target = SessionRef {
            endpoint: endpoint.endpoint.clone(),
            session_id: "saved-session".to_owned().try_into()?,
        };
        let approver: ProviderIdentity =
            serde_json::from_value(json!({"humanId":"different-saved-approver"}))?;
        let policy = ProviderRequestedPolicy {
            access: RouterAccess::WriteRestricted,
        };
        let mut store =
            ProviderOperationStore::open(&root_path.join("provider-operations.sqlite")).await?;
        store
            .record_session(&ProviderSessionRecord {
                target: target.clone(),
                working_directory: ProviderWorkingDirectory::try_from(
                    saved_directory.display().to_string(),
                )?,
                requested_policy: policy.clone(),
                created_by: approver.clone(),
                approver: approver.clone(),
                updated_at_ms: 1,
            })
            .await?;
        let binding = endpoint
            .channels
            .iter()
            .find_map(|channel| match channel {
                collaboration_protocol::ChannelDescription::ExternalProvider {
                    transport,
                    binding_id,
                    binding_generation,
                    runtime,
                    capabilities,
                } => Some(collaboration_protocol::ProviderBindingIdentity {
                    endpoint: endpoint.endpoint.clone(),
                    binding_id: binding_id.clone(),
                    runtime: runtime.clone(),
                    transport: *transport,
                    generation: collaboration_protocol::CodexGeneration {
                        service_epoch: client.identity().service_epoch.clone(),
                        generation: *binding_generation,
                    },
                    capabilities: capabilities.clone(),
                }),
                _ => None,
            })
            .ok_or("Cursor external provider binding missing")?;
        let observed_directory = root_path.join("source-api-observer");
        std::fs::create_dir(&observed_directory)?;
        std::fs::set_permissions(&observed_directory, std::fs::Permissions::from_mode(0o700))?;
        let manifest: collaboration_protocol::ServiceManifest =
            serde_json::from_slice(&std::fs::read(root_path.join("service.json"))?)?;
        std::fs::copy(
            root_path.join("service.json"),
            observed_directory.join("service.json"),
        )?;
        let observation_shutdown = tokio_util::sync::CancellationToken::new();
        let wire = recorded_api_listener::RecordedApiListener::start_on_unix(
            manifest.mcp.url,
            &observed_directory.join("control.sock"),
            observation_shutdown.clone(),
        )
        .await?;
        std::fs::set_permissions(
            observed_directory.join("control.sock"),
            std::fs::Permissions::from_mode(0o600),
        )?;
        let inspection = invoke_source(
            &json!({"serviceDirectory":observed_directory,
        "expectedService":endpoint.endpoint.service_id,"operation":{"kind":"clientInventory"}}),
            &invoking_directory,
        )
        .await?;
        assert!(
            inspection.status.success(),
            "source client inspection is read-only and must be supported: {}",
            String::from_utf8_lossy(&inspection.stdout)
        );
        let inspection: Value = serde_json::from_slice(&inspection.stdout)?;
        assert_eq!(inspection["reply"]["kind"], "clientInventory");
        assert!(
            inspection["reply"]["inventory"]["endpoints"]
                .as_array()
                .unwrap()
                .iter()
                .any(|description| description["endpoint"] == json!(binding.endpoint))
        );
        assert!(
            !provider_log.exists(),
            "client choices must not launch the provider"
        );
        let inspected=invoke_source(&json!({"serviceDirectory":observed_directory,
        "expectedService":endpoint.endpoint.service_id,"operation":{"kind":"providerInspect","binding":binding,"target":target}}),&invoking_directory).await?;
        assert!(
            inspected.status.success(),
            "provider Details inspection must reach the existing read API: {}",
            String::from_utf8_lossy(&inspected.stdout)
        );
        let inspected: Value = serde_json::from_slice(&inspected.stdout)?;
        assert_eq!(inspected["reply"]["kind"], "providerInspection");
        assert_eq!(inspected["reply"]["binding"], json!(binding));
        assert_eq!(inspected["reply"]["inspection"]["target"], json!(target));
        assert!(
            !provider_log.exists(),
            "Details inspection must not load/resume/create"
        );
        for (index, (action, expected_kind, method)) in [
            ("load", "managedLoad", "conversation_load"),
            ("resume", "managedResume", "conversation_resume"),
        ]
        .into_iter()
        .enumerate()
        {
            // Restore operations require an unloaded stored target. A loaded target
            // is tested separately as a real busy rejection, not silently retried.
            let target = if action == "resume" {
                let target = SessionRef {
                    endpoint: endpoint.endpoint.clone(),
                    session_id: "unloaded-resume-session".to_owned().try_into()?,
                };
                store
                    .record_session(&ProviderSessionRecord {
                        target: target.clone(),
                        working_directory: ProviderWorkingDirectory::try_from(
                            saved_directory.display().to_string(),
                        )?,
                        requested_policy: policy.clone(),
                        created_by: approver.clone(),
                        approver: approver.clone(),
                        updated_at_ms: 1,
                    })
                    .await?;
                target
            } else {
                target.clone()
            };
            let operation_id = OperationId::generate();
            let request = json!({"serviceDirectory":observed_directory,"expectedService":endpoint.endpoint.service_id,
        "operation":{"kind":"providerAction","operationId":operation_id,
            "binding":binding,"action":{"kind":action,"target":target}}});
            if action == "load" {
                for (field, value, expected_reason) in [
                    (
                        "/expectedService",
                        json!("00000000-0000-4000-8000-000000000099"),
                        "wrongService",
                    ),
                    (
                        "/operation/binding/generation/generation",
                        json!(999),
                        "staleBinding",
                    ),
                    (
                        "/operation/binding/runtime/provider",
                        json!(
                            if provider_kind == collaboration_protocol::ProviderKind::Cursor {
                                "claudeCode"
                            } else {
                                "cursor"
                            }
                        ),
                        "staleBinding",
                    ),
                    (
                        "/operation/action/target/sessionId",
                        json!("missing-stored-target"),
                        "sessionUnavailable",
                    ),
                ] {
                    let mut invalid = request.clone();
                    *invalid
                        .pointer_mut(field)
                        .ok_or("test request pointer missing")? = value;
                    let rejected = invoke_source(&invalid, &invoking_directory).await?;
                    assert_eq!(
                        rejected.status.code(),
                        Some(2),
                        "{}",
                        String::from_utf8_lossy(&rejected.stdout)
                    );
                    let reply: Value = serde_json::from_slice(&rejected.stdout)?;
                    assert_eq!(reply["reason"], expected_reason, "{reply}");
                    assert!(
                        !provider_log.exists(),
                        "rejection must precede provider session effects"
                    );
                    assert!(
                        store.inspect(&operation_id).await?.is_none(),
                        "rejected source request must not admit an operation"
                    );
                }
            }
            let output = invoke_source(&request, &invoking_directory).await?;
            assert!(
                output.status.success(),
                "source {action} must accept the typed request and complete: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let response: Value = serde_json::from_slice(&output.stdout)?;
            assert_eq!(response["kind"], "ready", "{response}");
            assert_eq!(response["reply"]["kind"], "providerAction", "{response}");
            let receipt = &response["reply"]["result"];
            assert_eq!(receipt["kind"], expected_kind, "{response}");
            if action == "load" {
                assert_eq!(receipt["result"]["kind"], "completed", "{response}");
                assert_eq!(receipt["result"]["operationId"], json!(operation_id));
                assert_eq!(receipt["result"]["target"], json!(target));
            } else {
                assert_eq!(
                    receipt["result"]["operation"]["operationId"],
                    json!(operation_id)
                );
                assert_eq!(receipt["result"]["operation"]["target"], json!(target));
                assert_eq!(
                    receipt["result"]["output"]["kind"], "available",
                    "{response}"
                );
                assert_eq!(
                    receipt["result"]["output"]["settlement"]["kind"], "resumed",
                    "{response}"
                );
            }
            let saved = store
                .session_record(&target)
                .await?
                .ok_or("loaded session record absent")?;
            assert_eq!(
                String::from(saved.working_directory.clone()),
                saved_directory.to_str().ok_or("saved path UTF8")?
            );
            assert_eq!(saved.requested_policy, policy);
            assert_eq!(saved.approver, approver);
            let human = ProviderIdentity::Human {
                human_id: collaboration_client::resolve_owner_human_id().await?,
            };
            let calls = wire
                .arguments_for(method)
                .into_iter()
                .filter(|call| call["operationId"] == json!(operation_id))
                .collect::<Vec<_>>();
            assert_eq!(
                calls.len(),
                1,
                "one exact source Load reaches the genuine API"
            );
            let submitted = &calls[0];
            assert_eq!(
                submitted["requestedBy"],
                json!(human),
                "source-resolved Human reaches the actual callable boundary"
            );
            assert_eq!(submitted["approver"], json!(approver));
            if action == "load" {
                assert_eq!(submitted["access"], "write-restricted");
            } else {
                assert_eq!(
                    submitted["requestedPolicy"],
                    json!({"access":"write-restricted"})
                );
            }
            assert_eq!(submitted["workingDirectory"], json!(saved_directory));
            assert_eq!(submitted["operationId"], json!(operation_id));
            assert_eq!(
                saved.created_by, approver,
                "Load must retain the original creator"
            );
            assert_ne!(
                human, saved.approver,
                "saved Approver must not become the invoking Human"
            );
            let operation = store
                .inspect(&operation_id)
                .await?
                .ok_or("caller operation not recorded")?;
            assert_eq!(operation.target.as_ref(), Some(&target));
            assert_eq!(operation.operation_id, operation_id);
            assert_eq!(
                operation.stage,
                collaboration_protocol::ProviderOperationStage::Terminal
            );
            assert_eq!(
                operation.effect,
                collaboration_protocol::ProviderOperationEffect::Applied
            );
            // A caller can inspect its retained operation without replaying the
            // mutation. The genuine API/store is the deciding readback owner.
            let readback_request = json!({"serviceDirectory":observed_directory,
            "expectedService":endpoint.endpoint.service_id,
            "operation":{"kind":"providerOperationShow","serviceEpoch":binding.generation.service_epoch,
                "operationId":operation_id}});
            let readback = invoke_source(&readback_request, &invoking_directory).await?;
            assert!(
                readback.status.success(),
                "{}",
                String::from_utf8_lossy(&readback.stdout)
            );
            let readback: Value = serde_json::from_slice(&readback.stdout)?;
            assert_eq!(readback["reply"]["kind"], "providerOperation");
            assert_eq!(readback["reply"]["operationId"], json!(operation_id));
            assert_eq!(readback["reply"]["result"]["kind"], "found");
            assert_eq!(
                readback["reply"]["result"]["operation"]["target"],
                json!(target)
            );
            assert_eq!(
                readback["reply"]["result"]["operation"]["operationId"],
                json!(operation_id)
            );
            let mut absent_request = readback_request.clone();
            absent_request["operation"]["operationId"] = json!(OperationId::generate());
            let absent = invoke_source(&absent_request, &invoking_directory).await?;
            assert!(absent.status.success());
            let absent: Value = serde_json::from_slice(&absent.stdout)?;
            assert_eq!(absent["reply"]["result"]["kind"], "failed");
            assert_eq!(absent["reply"]["result"]["failure"]["kind"], "notFound");
            assert_eq!(
                wire.arguments_for(method)
                    .into_iter()
                    .filter(|call| call["operationId"] == json!(operation_id))
                    .count(),
                1,
                "readback never resubmits"
            );
            let requests = std::fs::read_to_string(&provider_log)?
                .lines()
                .map(serde_json::from_str::<Value>)
                .collect::<Result<Vec<_>, _>>()?;
            assert_eq!(
                requests.len(),
                index + 1,
                "one caller action must dispatch once"
            );
            let last = requests.last().ok_or("provider request missing")?;
            assert_eq!(last["method"], format!("session/{action}"));
            assert_eq!(last["params"]["cwd"], json!(saved_directory));
            if action == "load" {
                let mut busy = request.clone();
                busy["operation"]["operationId"] = json!(OperationId::generate());
                busy["operation"]["action"]["kind"] = json!("resume");
                let rejected = invoke_source(&busy, &invoking_directory).await?;
                let reply: Value = serde_json::from_slice(&rejected.stdout)?;
                assert_eq!(reply["reply"]["result"]["kind"], "failed", "{reply}");
                assert_eq!(
                    reply["reply"]["result"]["failure"]["kind"], "busy",
                    "{reply}"
                );
                assert_eq!(reply["reply"]["result"]["failure"]["effect"], "none");
                assert_eq!(
                    std::fs::read_to_string(&provider_log)?.lines().count(),
                    1,
                    "an already loaded target must not dispatch another restore or fall back"
                );
            }
        }
        let create_id = OperationId::generate();
        let new_directory = root_path.join("explicit-new-project");
        std::fs::create_dir(&new_directory)?;
        let request = json!({"serviceDirectory":observed_directory,"expectedService":endpoint.endpoint.service_id,
        "operation":{"kind":"providerAction","operationId":create_id,"binding":binding,
            "action":{"kind":"create","directory":new_directory}}});
        let output = invoke_source(&request, &invoking_directory).await?;
        assert!(
            output.status.success(),
            "source managed create must complete: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let reply: Value = serde_json::from_slice(&output.stdout)?;
        let receipt = &reply["reply"]["result"];
        assert_eq!(receipt["kind"], "managedCreate", "{reply}");
        assert_eq!(receipt["result"]["kind"], "created", "{reply}");
        assert_eq!(receipt["result"]["operationId"], json!(create_id));
        let created: SessionRef = serde_json::from_value(receipt["result"]["target"].clone())?;
        assert_eq!(
            String::from(created.session_id.clone()),
            "created-by-source"
        );
        assert_eq!(created.endpoint, endpoint.endpoint);
        let record = store
            .session_record(&created)
            .await?
            .ok_or("new managed record absent")?;
        let human = ProviderIdentity::Human {
            human_id: collaboration_client::resolve_owner_human_id().await?,
        };
        assert_eq!(record.created_by, human);
        assert_eq!(record.approver, human);
        assert_eq!(
            record.requested_policy.access,
            RouterAccess::WorkspaceWrite,
            "only New uses its normal request policy"
        );
        assert_eq!(
            String::from(record.working_directory),
            new_directory.display().to_string()
        );
        let calls = wire.arguments_for("conversation_create");
        assert_eq!(calls.len(), 1, "one create, never a model prompt");
        assert_eq!(calls[0]["operationId"], json!(create_id));
        assert_eq!(calls[0]["workingDirectory"], json!(new_directory));
        let creator: ProviderIdentity = serde_json::from_value(calls[0]["createdBy"].clone())?;
        assert_eq!(creator, human);
        let requests = std::fs::read_to_string(&provider_log)?
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            requests.len(),
            3,
            "Load, Resume and New only; no prompt or fallback"
        );
        assert_eq!(requests[2]["method"], "session/new");
        assert_eq!(requests[2]["params"]["cwd"], json!(new_directory));
        let explicit_target = SessionRef {
            endpoint: endpoint.endpoint.clone(),
            session_id: "explicit-human-cli-load".to_owned().try_into()?,
        };
        store
            .record_session(&ProviderSessionRecord {
                target: explicit_target.clone(),
                working_directory: ProviderWorkingDirectory::try_from(
                    saved_directory.display().to_string(),
                )?,
                requested_policy: policy.clone(),
                created_by: approver.clone(),
                approver: approver.clone(),
                updated_at_ms: 1,
            })
            .await?;
        let explicit_operation = OperationId::generate();
        let explicit_human = json!({"humanId":"explicit-cli-human"});
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"));
        command
            .args(["conversation", "load", "--service-directory"])
            .arg(&observed_directory)
            .arg("--target")
            .arg(serde_json::to_string(&explicit_target)?)
            .arg("--cwd")
            .arg(&saved_directory)
            .args(["--access", "write-restricted", "--from"])
            .arg(explicit_human.to_string())
            .arg("--approver")
            .arg(serde_json::to_string(&approver)?)
            .arg("--operation-id")
            .arg(explicit_operation.as_str())
            .arg("--json")
            .env("HOME", &root_path)
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(15), command.output()).await??;
        assert!(
            output.status.success(),
            "public conversation Load must accept explicit Human: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let calls = wire
            .arguments_for("conversation_load")
            .into_iter()
            .filter(|call| call["operationId"] == json!(explicit_operation))
            .collect::<Vec<_>>();
        assert_eq!(
            calls.len(),
            1,
            "CLI Human Load must submit the caller operation once"
        );
        assert_eq!(calls[0]["requestedBy"], explicit_human);
        assert_eq!(calls[0]["approver"], json!(approver));
        let explicit_record = store
            .session_record(&explicit_target)
            .await?
            .ok_or("explicit CLI target missing")?;
        assert_eq!(explicit_record.requested_policy, policy);
        assert_eq!(explicit_record.approver, approver);
        store.close().await?;
        observation_shutdown.cancel();
        wire.finish().await?;
        runtime.shutdown().await?;
    }
    Ok(())
    }.await;
    result.expect("real source/Host/metadata restore scenario");
}

async fn invoke_source(
    request: &Value,
    directory: &std::path::Path,
) -> TestResult<std::process::Output> {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-sessions"));
    command
        .args(["--source-request-v1", &request.to_string()])
        .current_dir(directory)
        .env(
            "HOME",
            directory.parent().ok_or("fixture home parent missing")?,
        )
        .kill_on_drop(true);
    Ok(tokio::time::timeout(Duration::from_secs(15), command.output()).await??)
}
