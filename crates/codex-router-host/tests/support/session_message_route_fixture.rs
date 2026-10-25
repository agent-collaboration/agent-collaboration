//! ACP, peer, wake, subscription, and approval fixtures for Host composition proof.
use super::registered_route_project::{
    RegisteredRouteProject, assert_board_accepted, settled_creation,
};
use collaboration_client::{CollaborationClient, MessageSendRequest, PublicMessageContent};
use collaboration_protocol::{
    ConversationCreateRequest, ConversationLoadRequest, ConversationOperationSettlement,
    ConversationOperationWaitOutput, ConversationOperationWaitRequest, DeliveryDisposition,
    DeliveryEvidence, DeliveryOutcome, DeliveryShowRequest, EndpointRef, MessageDelivery,
    MessageText, OperationId, PositiveSeconds, ProviderRequestedPolicy, ProviderWorkingDirectory,
    RouterAccess, SessionId, SessionRef, ThreadSubscribeRequest, WakeSendRequest, WakeShowRequest,
};
use message_board::{
    Identity, ParticipantRole, PostId, PostReferences, PostText, ProtocolVersion, RequestId,
    SubscriptionMode, SubscriptionPolicyPatch, SubscriptionScope, SubscriptionTimingPatch,
    ThreadId, ThreadJoinRequest, ThreadPostRequest,
};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::{os::unix::fs::PermissionsExt as _, path::Path};

pub(super) fn provider_fixture(path: &Path) {
    std::fs::write(
        path,
        r#"#!/usr/bin/python3
import json,sys
session_id=sys.argv[1]
load_marker=sys.argv[2]
prompt_log=sys.argv[3]
request=json.loads(sys.stdin.readline())
print(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':{'protocolVersion':1,'agentCapabilities':{'loadSession':True},'agentInfo':{'name':'delivery-fixture','version':'1'},'_meta':{'steering':{'supported':True}}}})); sys.stdout.flush()
for line in sys.stdin:
 request=json.loads(line)
 method=request['method']
 if method=='session/new':
  result={'sessionId':session_id}
 elif method=='session/load':
  open(load_marker,'a').write('load\n')
  result={}
 elif method=='_session/steering':
  result={'outcome':'promptRequired'}
 elif method=='session/prompt':
  open(prompt_log,'a').write(json.dumps(request)+'\n')
  if 'request permission' in request['params']['prompt'][0]['text']:
   print(json.dumps({'jsonrpc':'2.0','id':91,'method':'session/request_permission','params':{'sessionId':session_id,'toolCall':{'toolCallId':'permission-tool'},'options':[{'optionId':'allow-once','name':'Allow once','kind':'allow_once'},{'optionId':'deny-once','name':'Deny once','kind':'reject_once'}]}})); sys.stdout.flush()
   decision=json.loads(sys.stdin.readline())
   assert decision['id']==91
   assert decision['result']['outcome']['outcome']=='selected'
   assert decision['result']['outcome']['optionId']=='allow-once'
   open(prompt_log,'a').write(json.dumps({'fixturePermissionDecision':decision})+'\n')
  result={'stopReason':'end_turn'}
 else:
  raise RuntimeError(method)
 print(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':result})); sys.stdout.flush()
 open(prompt_log,'a').write(json.dumps({'fixturePromptCompleted':request['id']})+'\n')
"#,
    )
    .expect("provider fixture");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .expect("provider executable");
}

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

pub(super) async fn post_thread_activity_for_sessions(
    client: &CollaborationClient,
    readers: [SessionRef; 2],
    inputs: RegisteredRouteProject,
) {
    let owner = inputs.project.actor.clone();
    let project = inputs.project.project_id.clone();
    assert_board_accepted(
        &settled_creation(|| client.workspace_create(inputs.workspace.clone())).await,
    );
    assert_board_accepted(&settled_creation(|| client.board_create(inputs.board.clone())).await);
    assert_board_accepted(
        &settled_creation(|| client.project_create(inputs.project.clone())).await,
    );
    assert_board_accepted(&client.role_grant(serde_json::from_value(json!({"requestId":RequestId::generate(),"protocolVersion":1,"actor":owner,"actingFor":null,"projectId":project,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":owner,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}})).expect("role request")).await.expect("route lead"));
    let root = PostId::generate();
    let thread = ThreadId::from(root.clone());
    assert_board_accepted(
        &client
            .thread_create(message_board::ThreadCreateRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                post_id: root,
                project_id: project,
                about: message_board::ThreadSubject::Project {
                    line: "Route discussion".to_owned().try_into().expect("subject"),
                },
                role: None,
                watch: false,
                actor: owner.clone(),
                acting_for: None,
                text: PostText::try_from("Route root".to_owned()).expect("root text"),
                references: PostReferences::try_from(Vec::new()).expect("references"),
                addressees: Vec::new(),
            })
            .await
            .expect("root post"),
    );
    for target in readers {
        let reader: Identity = serde_json::from_value(json!({"kind":"session", "session": target}))
            .expect("session reader");
        assert_board_accepted(
            &client
                .thread_join(ThreadJoinRequest {
                    request_id: RequestId::generate(),
                    protocol_version: ProtocolVersion::CURRENT,
                    thread: (thread.clone()).into(),
                    actor: reader.clone(),
                    role: ParticipantRole::Participant,
                    watch: true,
                    mode: None,
                    when_idle: None,
                    replace: None,
                    note: None,
                })
                .await
                .expect("reader joined"),
        );
        assert_board_accepted(
            &client
                .thread_subscribe(ThreadSubscribeRequest {
                    request_id: RequestId::generate(),
                    protocol_version: ProtocolVersion::CURRENT,
                    actor: reader,
                    acting_for: None,
                    scope: SubscriptionScope::thread(thread.clone()).into(),
                    policy: SubscriptionPolicyPatch {
                        mode: Some(SubscriptionMode::Deliver),
                        timing: SubscriptionTimingPatch {
                            quiet_seconds: Some(0),
                            cap_seconds: Some(0),
                        },
                        ..SubscriptionPolicyPatch::default()
                    },
                })
                .await
                .expect("session subscription"),
        );
    }
    assert_board_accepted(
        &client
            .thread_post(ThreadPostRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                post_id: PostId::generate(),
                placement: message_board::PostTarget::Thread {
                    thread: thread.into(),
                },
                actor: owner,
                acting_for: None,
                text: PostText::try_from("Route reply".to_owned()).expect("reply text"),
                references: PostReferences::try_from(Vec::new()).expect("references"),
                addressees: Vec::new(),
            })
            .await
            .expect("reply post"),
    );
}

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
#[allow(dead_code)]
mod provider_conversation_submission;
use provider_conversation_submission::{
    spawn_provider_prompt, submit_provider_create, submit_provider_load,
};

#[path = "session_message_approval_fixture.rs"]
mod approval_fixture;
pub(super) use approval_fixture::prompt_and_approve_from_peer_provider;
