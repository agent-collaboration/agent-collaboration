//! ACP and peer fixtures for the Host delivery composition.
use collaboration_client::CollaborationClient;
use collaboration_contracts::{
    AttemptEvidenceSink, DeliveryFuture, DeliveryPrecondition, LoadPolicy, SessionMessageDelivery,
    layer_zero::{DeliveryRequest, PreparedPush},
};
use collaboration_protocol::{
    ConversationCreateRequest, ConversationLoadRequest, ConversationOperationSettlement,
    ConversationOperationWaitOutput, ConversationOperationWaitRequest, EndpointRef,
    MessageDelivery, MessageText, OperationId, PositiveSeconds, ProviderRequestedPolicy,
    ProviderWorkingDirectory, RouterAccess, SessionId, SessionRef,
};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::{os::unix::fs::PermissionsExt as _, path::Path};

#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "Unexpected native fixture frames and setup failures fail the route proof immediately."
)]
pub(super) async fn native_delivery_fixture(
    root: &Path,
) -> (
    std::sync::Arc<codex_native_integration::NativeSchemaExport>,
    tokio::task::JoinHandle<()>,
) {
    use futures_util::{SinkExt as _, StreamExt as _};
    use tokio_tungstenite::tungstenite::Message;
    let mut definitions = serde_json::Map::new();
    for name in [
        "ThreadRead",
        "ThreadList",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
    ] {
        definitions.insert(format!("{name}Params"), json!({"type":"object"}));
        definitions.insert(format!("{name}Response"), json!({"type":"object"}));
    }
    let schema = json!({"definitions":{"v2":definitions}});
    let generator = root.join("schema-generator");
    std::fs::write(
        &generator,
        format!("#!/bin/sh\ncat > \"$5/codex_app_server_protocol.schemas.json\" <<'SCHEMA'\n{schema}\nSCHEMA\n"),
    ).expect("native schema generator");
    std::fs::set_permissions(&generator, std::fs::Permissions::from_mode(0o700))
        .expect("native schema generator permissions");
    let identity = codex_native_integration::executable_identity(&generator)
        .await
        .expect("generator identity");
    let export = codex_native_integration::NativeSchemaExport::generate(
        &identity,
        &root.join("native-schema"),
    )
    .await
    .expect("native schema export");
    let listener =
        tokio::net::UnixListener::bind(root.join("backend.sock")).expect("native socket");
    let backend = tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                result = connections.join_next(), if !connections.is_empty() => {
                    result.expect("connection joined").expect("native fixture connection");
                }
                accepted = listener.accept() => {
                    let (stream, _) = accepted.expect("accept native connection");
                    connections.spawn(async move {
                        let mut socket = tokio_tungstenite::accept_async(stream)
                            .await.expect("native websocket");
                        while let Some(Ok(Message::Text(text))) = socket.next().await {
                            let request: serde_json::Value = serde_json::from_str(&text)
                                .expect("native request");
                            let Some(id) = request.get("id") else { continue };
                            let result = match request["method"].as_str() {
                                Some("initialize") => json!({}),
                                Some("thread/list" | "thread/loaded/list") => json!({"data":[],"nextCursor":null}),
                                Some("thread/read") => json!({
                                    "thread":{"id":request["params"]["threadId"],"cwd":"/work","status":{"type":if request["params"]["threadId"] == "unloaded" { "notLoaded" } else { "idle" }}}
                                }),
                                Some("turn/start") => {
                                    assert_ne!(request["params"]["threadId"], "unloaded");
                                    assert!(request["params"]["input"][0]["text"]
                                        .as_str().expect("delivery line").contains("routed message"));
                                    json!({"turn":{"id":"delivered-turn"}})
                                }
                                Some("thread/resume") => panic!("delivery must not resume a Codex thread"),
                                _ => json!({}),
                            };
                            socket.send(Message::Text(json!({"id":id,"result":result})
                                .to_string().into())).await.expect("native response");
                        }
                    });
                }
            }
        }
    });
    (std::sync::Arc::new(export), backend)
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
pub(super) fn provider_fixture(path: &Path) {
    node_test_fixture::write_node_fixture(
        path,
        "session-message-route-agent.ts",
        &serde_json::json!(null),
    )
    .expect("provider fixture");
}

#[derive(Default)]
struct RouteEvidence(
    tokio::sync::Mutex<
        Vec<
            agent_automation::RouteEffectEvidence<
                SessionRef,
                collaboration_protocol::CodexGeneration,
            >,
        >,
    >,
);

impl AttemptEvidenceSink for RouteEvidence {
    fn record(
        &self,
        evidence: agent_automation::RouteEffectEvidence<
            SessionRef,
            collaboration_protocol::CodexGeneration,
        >,
    ) -> DeliveryFuture<'_, ()> {
        Box::pin(async {
            self.0.lock().await.push(evidence);
            Ok(())
        })
    }
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed route fixtures.")]
pub(crate) async fn deliver(
    runtime: &codex_router_host::CollaborationRuntime,
    target: SessionRef,
) -> collaboration_protocol::DeliveryReceipt {
    let evidence = RouteEvidence::default();
    let item = uuid::Uuid::now_v7().to_string();
    let line = format!(
        "✉️ sender · \"routed message\" · router://{}/push/{item}",
        String::from(target.endpoint.service_id.clone()),
    );
    let load_policy = if String::from(target.endpoint.endpoint_id.clone()) == "codex-local" {
        LoadPolicy::MayLoad
    } else {
        LoadPolicy::LoadedOnly
    };
    let request = DeliveryRequest {
        payload: PreparedPush {
            push_id: item.clone().try_into().expect("push id"),
            line: MessageText::try_from(line).expect("message"),
            load_policy,
        },
        target,
        mode: MessageDelivery::Auto,
        precondition: DeliveryPrecondition::Unpinned,
        correlation: item.try_into().expect("delivery correlation"),
        attempt: agent_automation::AttemptId::generate(),
    };
    let routed = runtime
        .session_delivery()
        .expect("Host composition")
        .deliver(request, &evidence)
        .await
        .expect("router receipt")
        .into_receipt();
    if matches!(
        routed.outcome,
        collaboration_protocol::DeliveryOutcome::Started
            | collaboration_protocol::DeliveryOutcome::PeerMessageWritten
    ) {
        assert!(
            !evidence.0.lock().await.is_empty(),
            "chosen carrier recorded attempt evidence"
        );
    }
    routed
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

use crate::provider_conversation_submission;
use provider_conversation_submission::{submit_provider_create, submit_provider_load};
