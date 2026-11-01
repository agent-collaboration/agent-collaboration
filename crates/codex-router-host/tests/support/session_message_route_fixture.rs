//! ACP, peer, wake, subscription, and approval fixtures for Host composition proof.
use collaboration_client::{CollaborationClient, MessageSendRequest, PublicMessageContent};
use collaboration_protocol::{
    ConversationCreateRequest, ConversationLoadRequest, ConversationOperationSettlement,
    ConversationOperationWaitOutput, ConversationOperationWaitRequest, DeliveryDisposition,
    DeliveryEvidence, DeliveryOutcome, DeliveryShowRequest, EndpointRef, MessageDelivery,
    MessageText, OperationId, PositiveSeconds, ProviderRequestedPolicy, ProviderWorkingDirectory,
    RouterAccess, SessionId, SessionRef, WakeSendRequest, WakeShowRequest,
};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::{os::unix::fs::PermissionsExt as _, path::Path};

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
pub(super) fn provider_fixture(path: &Path) {
    node_test_fixture::write_node_fixture(
        path,
        "session-message-route-agent.ts",
        &serde_json::json!(null),
    )
    .expect("provider fixture");
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
pub(super) fn message(target: SessionRef) -> MessageSendRequest {
    MessageSendRequest {
        item_id: None,
        target,
        message: PublicMessageContent::HumanUser {
            text: MessageText::try_from("routed message".to_owned()).expect("message"),
        },
        delivery: MessageDelivery::Auto,
        generation_guard: None,
    }
}

#[expect(clippy::panic, reason = "Fail an unexpected test outcome.")]
pub(super) async fn wait_for_prompt_text(path: &Path, text: &str) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(observed_text) = std::fs::read_to_string(path).ok().and_then(|log| {
                log.lines()
                    .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                    .find(|entry| provider_prompt_contains(entry, text))
                    .and_then(|entry| {
                        entry
                            .get("params")?
                            .get("prompt")?
                            .as_array()?
                            .iter()
                            .find_map(|part| {
                                part.get("text")?
                                    .as_str()
                                    .filter(|value| value.contains(text))
                                    .map(str::to_owned)
                            })
                    })
            }) {
                break observed_text;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "provider prompt log deadline; log: {}",
            std::fs::read_to_string(path).unwrap_or_default()
        )
    })
}

fn provider_prompt_contains(entry: &serde_json::Value, text: &str) -> bool {
    entry.get("method").and_then(serde_json::Value::as_str) == Some("session/prompt")
        && entry
            .get("params")
            .and_then(|params| params.get("prompt"))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|parts| {
                parts.iter().any(|part| {
                    part.get("text")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|value| value.contains(text))
                })
            })
}

#[expect(clippy::panic, reason = "Fail an unexpected test outcome.")]
pub(super) async fn wait_for_completed_provider_prompts(path: &Path, expected_count: usize) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let completed_count = std::fs::read_to_string(path)
                .unwrap_or_default()
                .lines()
                .filter(|line| line.contains("fixturePromptCompleted"))
                .count();
            if completed_count >= expected_count {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "provider fixture completion deadline; log: {}",
            std::fs::read_to_string(path).unwrap_or_default()
        )
    });
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
pub(super) fn publish_peer(registry: &Path, session_id: &SessionId, protocol: u64, socket: &Path) {
    let process_id = std::process::id();
    std::fs::write(
        registry.join(format!("{process_id}.json")),
        json!({
            "pid": process_id,
            "sessionId": String::from(session_id.clone()),
            "status": "busy",
            "peerProtocol": protocol,
            "messagingSocketPath": socket,
        })
        .to_string(),
    )
    .expect("peer registry record");
    let digest = Sha256::digest(socket.to_str().expect("socket path").as_bytes());
    let key = registry.join(format!("{process_id}.{digest:x}.key"));
    std::fs::write(
        &key,
        json!({"peerToken":"0123456789abcdef0123456789abcdef"}).to_string(),
    )
    .expect("peer key");
    std::fs::set_permissions(key, std::fs::Permissions::from_mode(0o600))
        .expect("private peer key");
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
#[expect(clippy::panic, reason = "Fail an unexpected test outcome.")]
pub(super) async fn create_provider_target(
    client: &CollaborationClient,
    endpoint: EndpointRef,
    actor: SessionRef,
    working_directory: &Path,
) -> SessionRef {
    let generation = client
        .list_endpoints()
        .await
        .expect("catalog")
        .endpoints
        .into_iter()
        .find(|entry| entry.endpoint == endpoint)
        .expect("provider endpoint")
        .channels
        .into_iter()
        .find_map(|channel| match channel {
            collaboration_protocol::ChannelDescription::ExternalProvider {
                binding_generation,
                ..
            } => Some(binding_generation),
            _ => None,
        })
        .expect("binding generation");
    let operation_id = OperationId::generate();
    submit_provider_create(
        client,
        ConversationCreateRequest {
            project_id: None,
            settings: None,
            operation_id: operation_id.clone(),
            endpoint,
            generation: Some(collaboration_protocol::CodexGeneration {
                service_epoch: client.identity().service_epoch.clone(),
                generation,
            }),
            working_directory: ProviderWorkingDirectory::try_from(
                working_directory.display().to_string(),
            )
            .expect("working directory"),
            created_by: (actor.clone()).into(),
            approver: (actor).into(),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
        },
    )
    .await
    .expect("create admitted");
    let waited = client
        .wait_for_provider_conversation_operation(ConversationOperationWaitRequest {
            operation_id,
            timeout_seconds: PositiveSeconds::try_from(3).expect("timeout"),
        })
        .await
        .expect("create settled");
    match waited.output {
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::Created { target, .. },
        } => target,
        other => panic!("provider create did not settle: {other:?}"),
    }
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
pub(super) async fn load_provider_target(
    client: &CollaborationClient,
    target: SessionRef,
    actor: SessionRef,
    working_directory: &Path,
) {
    let generation = client
        .list_endpoints()
        .await
        .expect("catalog")
        .endpoints
        .into_iter()
        .find(|entry| entry.endpoint == target.endpoint)
        .expect("provider endpoint")
        .channels
        .into_iter()
        .find_map(|channel| match channel {
            collaboration_protocol::ChannelDescription::ExternalProvider {
                binding_generation,
                ..
            } => Some(binding_generation),
            _ => None,
        })
        .expect("binding generation");
    let operation_id = OperationId::generate();
    submit_provider_load(
        client,
        ConversationLoadRequest {
            operation_id: operation_id.clone(),
            target,
            generation: Some(collaboration_protocol::CodexGeneration {
                service_epoch: client.identity().service_epoch.clone(),
                generation,
            }),
            working_directory: ProviderWorkingDirectory::try_from(
                working_directory.display().to_string(),
            )
            .expect("working directory"),
            requested_by: actor.clone().into(),
            approver: actor.into(),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
        },
    )
    .await
    .expect("explicit provider load admitted");
    let waited = client
        .wait_for_provider_conversation_operation(ConversationOperationWaitRequest {
            operation_id,
            timeout_seconds: PositiveSeconds::try_from(3).expect("timeout"),
        })
        .await
        .expect("provider load settled");
    assert!(matches!(
        waited.output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::Loaded { .. }
        }
    ));
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
pub(super) async fn send_and_wait_wake(client: &CollaborationClient, target: SessionRef) {
    // Omitted expiry ends a day after due; a fixed historical date is already expired.
    let request: WakeSendRequest = serde_json::from_value(json!({
        "operationId": OperationId::generate(),
        "message": {
            "target": target,
            "content": {"kind":"humanUser","text":"wake through provider"},
            "delivery": "auto",
            "generationGuard": null
        },
        "timing": {"kind":"at","at":chrono::Utc::now()},
        "requestedBy":{"kind":"owner"},"replace":false,"expiry": {"kind":"none"}
    }))
    .expect("wake request");
    let wake = client.send_wakeup(request).await.expect("wake accepted");
    let delivery_id = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let snapshot = client
                .read_wakeup(WakeShowRequest {
                    wakeup_id: wake.definition.wakeup_id.clone(),
                })
                .await
                .expect("wake inspection");
            if let Some(delivery_id) = snapshot.pending_delivery_id {
                break delivery_id;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("wake fire deadline");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let delivery = client
                .read_delivery(DeliveryShowRequest {
                    delivery_id: delivery_id.clone(),
                })
                .await
                .expect("delivery inspection");
            if matches!(delivery.disposition, DeliveryDisposition::Accepted) {
                assert!(matches!(delivery.evidence,
                    DeliveryEvidence::Accepted { receipt, .. }
                    if receipt.outcome == DeliveryOutcome::Started
                        && receipt.reachability == Some(collaboration_protocol::SessionReachability::ProviderAcp)));
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("wake delivery deadline");
}

#[path = "provider_conversation_submission.rs"]
mod provider_conversation_submission;
use provider_conversation_submission::{
    spawn_provider_prompt, submit_provider_create, submit_provider_load,
};

#[path = "session_message_approval_fixture.rs"]
mod approval_fixture;
pub(super) use approval_fixture::prompt_and_approve_from_peer_provider;
