//! ACP, peer, wake, subscription, and approval fixtures for Host composition proof.
use collaboration_client::CollaborationClient;
use collaboration_protocol::{
    ConversationCreateRequest, ConversationOperationSettlement, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, EndpointRef, OperationId, PositiveSeconds,
    ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess, SessionRef,
};
use std::path::Path;

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
pub(super) fn provider_fixture(path: &Path) {
    node_test_fixture::write_node_fixture(
        path,
        "session-message-route-agent.ts",
        &serde_json::json!(null),
    )
    .expect("provider fixture");
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
#[expect(clippy::panic, reason = "Fail an unexpected test outcome.")]
pub(super) async fn create_provider_target(
    client: &CollaborationClient,
    endpoint: EndpointRef,
    actor: SessionRef,
    working_directory: &Path,
) -> SessionRef {
    let description = client
        .list_endpoints()
        .await
        .expect("catalog")
        .endpoints
        .into_iter()
        .find(|entry| entry.endpoint == endpoint)
        .expect("provider endpoint");
    let availability = description.availability;
    let generation = description
        .channels
        .into_iter()
        .find_map(|channel| match channel {
            collaboration_protocol::ChannelDescription::ExternalProvider {
                binding_generation,
                ..
            } => Some(binding_generation),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!("binding generation for {endpoint:?}; availability: {availability:?}")
        });
    let operation_id = OperationId::generate();
    submit_provider_create(
        client,
        ConversationCreateRequest {
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

use crate::provider_conversation_submission;
use provider_conversation_submission::{spawn_provider_prompt, submit_provider_create};

#[path = "session_message_approval_fixture.rs"]
mod approval_fixture;
pub(super) use approval_fixture::prompt_and_approve_from_peer_provider;
