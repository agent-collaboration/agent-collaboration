//! N4 node taps: a session command in, prompt contents out to the ACP client (N6).
//!
//! Each case drives the real `HostSessionCommandPort` over the real route,
//! supervisor, runtime and event hub against an owned ACP behavior fixture. What
//! N4 produced is read from what the fixture agent received on its wire, and what
//! N4 consumed from N5 is read from the hub.

use super::*;
use crate::{
    ExternalProviderBinding, ExternalProviderLaunch, ExternalProviderRuntime, LiveSessionOwnership,
    LiveSessionOwnershipCheck,
};
use codex_router_test_support::node_taps::{
    self, CatalogRowExpectation, EdgeParty, MessageFamily, ProtocolNode, RecordLayer, TapCase,
};
use collaboration_contracts::DeliveryFuture;
use collaboration_protocol::{
    ChannelDescription, CodexGeneration, EndpointAvailability, EndpointDescription,
    GenerationNumber, NonEmptyText, ObservationTimestamp, ProviderBindingId,
    ProviderBindingIdentity, ProviderCapabilities, ProviderCapability, ProviderCapabilityEvidence,
    ProviderCapabilityName, ProviderCapabilityStatus, ProviderKind, ProviderRuntimeIdentity,
    ProviderTransport, ProviderWorkingDirectory, RouterAccess,
};
use collaboration_runtime::{EndpointDirectory, ProviderSessionRecord};
use message_board::SessionRef as BoardSessionRef;
use session_event_model::{PromptContent, ProviderSubmission, SettingApplicationIntent};
use std::path::PathBuf;

const SERVICE_ID: &str = "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89";
const SCRIPTED_AGENT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/scripted-acp-agent.ts"
);
/// The ACP client numbers each request with a fresh UUID, so the id is the one generated field of a
/// recorded request; the placeholder stands for it.
const GENERATED_REQUEST_ID: &str = "<volatile:id>";

const BEHAVIOR_AGENT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../acp-client-runtime/tests/support/behavior-agent.ts"
);

struct NoLivePeer;

impl LiveSessionOwnershipCheck for NoLivePeer {
    fn check<'a>(
        &'a self,
        _target: &'a collaboration_protocol::SessionRef,
    ) -> DeliveryFuture<'a, LiveSessionOwnership> {
        Box::pin(async { Ok(LiveSessionOwnership::NotLive) })
    }
}

struct CommandPortRig {
    _root: tempfile::TempDir,
    receipt: PathBuf,
    port: HostSessionCommandPort,
    hub: Arc<ProviderSessionEventHub>,
    supervisor: Arc<ExternalProviderSupervisor>,
    board_target: BoardSessionRef,
    actor: Identity,
}

fn protocol_target(session_id: &str) -> collaboration_protocol::SessionRef {
    collaboration_protocol::SessionRef {
        endpoint: collaboration_protocol::EndpointRef {
            service_id: SERVICE_ID.to_owned().try_into().expect("service ID"),
            endpoint_id: EndpointId::try_from("cursor-local".to_owned()).expect("endpoint ID"),
        },
        session_id: SessionId::try_from(session_id.to_owned()).expect("session ID"),
    }
}

fn provider_binding(target: &collaboration_protocol::SessionRef) -> ProviderBindingIdentity {
    ProviderBindingIdentity {
        endpoint: target.endpoint.clone(),
        binding_id: ProviderBindingId::try_from("n4-tap-binding".to_owned()).expect("binding ID"),
        runtime: ProviderRuntimeIdentity {
            provider: ProviderKind::Cursor,
            runtime_name: NonEmptyText::try_from("n4-tap".to_owned()).expect("runtime name"),
            runtime_version: None,
        },
        transport: ProviderTransport::StdioAcp,
        generation: CodexGeneration {
            service_epoch: "1ff962c5-7fa3-4c18-a5ca-1bbe8db09e80"
                .to_owned()
                .try_into()
                .expect("service epoch"),
            generation: GenerationNumber::try_from(1).expect("generation"),
        },
        capabilities: ProviderCapabilities::try_from(vec![ProviderCapability {
            name: ProviderCapabilityName::Prompt,
            status: ProviderCapabilityStatus::Supported,
            evidence: ProviderCapabilityEvidence::Advertised,
        }])
        .expect("capabilities"),
    }
}

fn endpoint_directory(binding: &ProviderBindingIdentity) -> EndpointDirectory {
    let directory = EndpointDirectory::new(binding.endpoint.service_id.clone());
    directory
        .publish(EndpointDescription {
            endpoint: binding.endpoint.clone(),
            label: NonEmptyText::try_from("Cursor".to_owned()).expect("label"),
            availability: EndpointAvailability::Available {
                observed_at: ObservationTimestamp::try_from("2026-09-24T12:00:00Z".to_owned())
                    .expect("time"),
            },
            channels: vec![ChannelDescription::ExternalProvider {
                transport: ProviderTransport::StdioAcp,
                binding_id: binding.binding_id.clone(),
                binding_generation: binding.generation.generation,
                runtime: binding.runtime.clone(),
                capabilities: binding.capabilities.clone(),
                auto_realization: None,
            }],
        })
        .expect("endpoint");
    directory
}

enum AgentFixture {
    Behavior {
        agent_case: &'static str,
        with_release_file: bool,
    },
    ScriptedSteering,
}

fn scripted_steering_launch(receipt: &std::path::Path) -> ExternalProviderLaunch {
    let steps = serde_json::json!([
        {"action":"expect_request","requestName":"init","method":"initialize","params":{}},
        {"action":"respond","requestName":"init","result":{
            "protocolVersion":1,"agentCapabilities":{},
            "agentInfo":{"name":"n4-steering-fixture","version":"1"},
            "_meta":{"steering":{"supported":true}}}},
        {"action":"expect_request","requestName":"new","method":"session/new","params":{}},
        {"action":"respond","requestName":"new","result":{"sessionId":"fixture-session"}},
        {"action":"expect_request","requestName":"prompt","method":"session/prompt","params":{}},
        {"action":"expect_request","requestName":"steer","method":"_session/steering","params":{},
            "recordPath":receipt},
        {"action":"respond","requestName":"steer","result":{"outcome":"injected"}},
        {"action":"respond","requestName":"prompt","result":{"stopReason":"end_turn"}},
    ]);
    ExternalProviderLaunch {
        executable: "node".into(),
        arguments: vec![SCRIPTED_AGENT.to_owned(), steps.to_string()],
        environment: vec![],
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
    }
}

async fn command_port_rig(fixture: AgentFixture) -> CommandPortRig {
    let root = tempfile::tempdir().expect("rig root");
    let receipt = root.path().join("wire.jsonl");
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    let hub = Arc::new(ProviderSessionEventHub::new(Arc::clone(&store)));
    let seed_target = protocol_target("session-1");
    let binding = provider_binding(&seed_target);
    let board_endpoint: SessionEndpointRef =
        serde_json::from_value(serde_json::to_value(&seed_target.endpoint).expect("endpoint JSON"))
            .expect("board endpoint");
    let behavior: Arc<dyn acp_client_runtime::ProviderBehavior> = match fixture {
        AgentFixture::Behavior { .. } => Arc::new(acp_provider_adapters::CursorAcpBehavior),
        AgentFixture::ScriptedSteering => Arc::new(acp_provider_adapters::StandardAcpBehavior),
    };
    let launch = match fixture {
        AgentFixture::Behavior {
            agent_case,
            with_release_file,
        } => {
            let mut arguments = vec![
                BEHAVIOR_AGENT.to_owned(),
                receipt.display().to_string(),
                agent_case.to_owned(),
                "cursor_login".to_owned(),
            ];
            if with_release_file {
                arguments.push(root.path().join("release.sentinel").display().to_string());
            }
            ExternalProviderLaunch {
                executable: "node".into(),
                arguments,
                environment: vec![],
                persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
            }
        }
        AgentFixture::ScriptedSteering => scripted_steering_launch(&receipt),
    };
    let runtime = ExternalProviderRuntime::initialize_with_mcp_http_and_hub(
        launch,
        acp_client_runtime::ProviderModelPicker::Standard,
        "fixture",
        "http://127.0.0.1:1/mcp",
        Arc::clone(&hub),
        board_endpoint,
        behavior,
    )
    .await
    .expect("provider runtime");
    let provider_session = runtime
        .create_session(root.path().into())
        .await
        .expect("session/new");
    let target = protocol_target(&provider_session);
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from(
                root.path().display().to_string(),
            )
            .expect("cwd"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: target.clone().into(),
            approver: target.clone().into(),
            updated_at_ms: 1,
        })
        .await
        .expect("record session");
    let supervisor = Arc::new(
        ExternalProviderSupervisor::new(
            vec![ExternalProviderBinding {
                identity: binding.clone(),
                runtime,
            }],
            Arc::clone(&store),
        )
        .expect("supervisor"),
    );
    let delivery = Arc::new(ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        endpoint_directory(&binding),
        Arc::clone(&supervisor),
        Arc::clone(&store),
        Arc::new(NoLivePeer),
        std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
    ));
    let port = HostSessionCommandPort::new(
        Arc::clone(&supervisor),
        delivery,
        Arc::clone(&hub),
        store,
        Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH)),
    );
    let board_target: BoardSessionRef =
        serde_json::from_value(serde_json::to_value(&target).expect("target JSON"))
            .expect("board target");
    let actor = ProviderIdentity::Session(target)
        .to_board_identity()
        .expect("board actor");
    CommandPortRig {
        _root: root,
        receipt,
        port,
        hub,
        supervisor,
        board_target,
        actor,
    }
}

fn text_and_link() -> ProviderSubmission {
    ProviderSubmission::plain(vec![
        PromptContent::text("Read".into()).expect("text"),
        PromptContent::resource_link(
            "https://example.test/context".into(),
            "context".into(),
            None,
        )
        .expect("resource link"),
    ])
}

/// Waits, event by event, for the next `TurnEnded` on the Session.
async fn wait_for_turn_end(rig: &CommandPortRig) {
    use collaboration_contracts::SessionEventHub as _;
    let mut attached = rig
        .hub
        .attach(rig.board_target.clone())
        .await
        .expect("attach");
    if attached
        .snapshot
        .iter()
        .any(|event| matches!(event.event, SessionEvent::TurnEnded { .. }))
    {
        return;
    }
    while !matches!(
        attached.receiver.recv().await.expect("hub event").event,
        SessionEvent::TurnEnded { .. }
    ) {}
}

/// Every ACP request the fixture agent received on its wire, as N4 outputs to N6: the kind
/// N4's catalog row names and the whole JSON-RPC request (`jsonrpc`, `id`, `method`, `params`).
/// Reads the whole receipt.
fn received_by_agent(rig: &CommandPortRig) -> Vec<(String, serde_json::Value)> {
    std::fs::read_to_string(&rig.receipt)
        .expect("agent receipt")
        .lines()
        .filter_map(|line| {
            let message: serde_json::Value = serde_json::from_str(line).ok()?;
            let params = message["params"].clone();
            let kind = match message["method"].as_str()? {
                "session/prompt" => format!("PromptContents[{}]", block_kinds(&params)?),
                "_session/steering" => format!("SteerContents[{}]", block_kinds(&params)?),
                "session/set_config_option" => format!(
                    "session/set_config_option:{}={}",
                    params["configId"].as_str()?,
                    params["value"].as_str()?
                ),
                other => other.to_owned(),
            };
            Some((kind, node_taps::stable(message, &["id"])))
        })
        .collect()
}

fn block_kinds(params: &serde_json::Value) -> Option<String> {
    let kinds: Vec<&str> = params["prompt"]
        .as_array()?
        .iter()
        .filter_map(|block| block["type"].as_str())
        .collect();
    Some(kinds.join(","))
}

/// Every `TurnStarted` the hub holds for the Session, as N4 consumed it from N5.
async fn turn_started_seen_by_n4(rig: &CommandPortRig) -> Vec<serde_json::Value> {
    use collaboration_contracts::SessionEventHub as _;
    rig.hub
        .attach(rig.board_target.clone())
        .await
        .expect("attach")
        .snapshot
        .into_iter()
        .filter(|event| matches!(event.event, SessionEvent::TurnStarted { .. }))
        .map(|event| {
            node_taps::stable(
                serde_json::to_value(&event.event).expect("event JSON"),
                &["turnId"],
            )
        })
        .collect()
}

/// The whole command N4 took in, as its full `Debug` text (the command has no serde form).
fn consumed_command(case: &mut TapCase, kind: &str, command: &impl std::fmt::Debug) {
    case.consumed_with(
        RecordLayer::TypedInput,
        MessageFamily::SessionCommand,
        EdgeParty::Node(ProtocolNode::N1CliFace),
        kind,
        serde_json::json!({"debug": format!("{command:#?}")}),
    );
}

/// The whole `session/prompt` request the fixture agent should receive.
fn prompt_request(id: &str, blocks: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/prompt",
        "params": {"sessionId": "session-1", "prompt": blocks},
    })
}

/// N4's prompt contents toward N6, observed where they surface: the request the agent received.
/// N4 → N6 is Router vocabulary between parts, so the record is a typed outcome; the ACP frame
/// itself is N6's output and is recorded at the wire by N6's own taps.
fn record_agent_wire(case: &mut TapCase, rig: &CommandPortRig) {
    for (kind, message) in received_by_agent(rig) {
        case.produced_with(
            RecordLayer::TypedOutcome,
            MessageFamily::PromptContents,
            EdgeParty::Node(ProtocolNode::N6AcpClient),
            &kind,
            message,
        );
    }
}

async fn prompt_case(
    case_name: &str,
    catalog_row: &str,
    submission: ProviderSubmission,
    expected_wire: Option<(&str, serde_json::Value)>,
) -> Result<(), CommandFailure> {
    let rig = command_port_rig(AgentFixture::Behavior {
        agent_case: "settings",
        with_release_file: false,
    })
    .await;
    let mut case = TapCase::new(ProtocolNode::N4CommandPort, case_name, catalog_row);
    let command = PromptSessionCommand {
        target: rig.board_target.clone(),
        input_id: InputId::new("n4-tap-input").expect("input id"),
        submission,
        actor: rig.actor.clone(),
    };
    consumed_command(&mut case, "Prompt", &command);
    let outcome = rig.port.prompt(command).await;
    if outcome.is_ok() {
        wait_for_turn_end(&rig).await;
        for started in turn_started_seen_by_n4(&rig).await {
            case.consumed_with(
                RecordLayer::TypedInput,
                MessageFamily::SessionEvent,
                EdgeParty::Node(ProtocolNode::N5EventHub),
                "TurnStarted",
                started,
            );
        }
    }
    record_agent_wire(&mut case, &rig);
    let mut row = CatalogRowExpectation::new();
    if let Some((expected_wire, expected_message)) = expected_wire {
        row = row.produces_with(
            MessageFamily::PromptContents,
            EdgeParty::Node(ProtocolNode::N6AcpClient),
            expected_wire,
            expected_message,
        );
    }
    case.finish(&row).assert_settled();
    rig.supervisor
        .shutdown()
        .await
        .expect("supervisor shutdown");
    outcome.map(|_| ())
}

#[tokio::test]
async fn node_tap_prompt_text_and_resource_link_reach_the_acp_client_in_order() {
    prompt_case(
        "prompt_text_and_resource_link",
        "N4: Session command (prompt) -> prompt contents to N6; waits for TurnStarted from N5",
        text_and_link(),
        Some((
            "PromptContents[text,resource_link]",
            prompt_request(
                GENERATED_REQUEST_ID,
                serde_json::json!([
                    {"type": "text", "text": "Read"},
                    {"type": "resource_link", "uri": "https://example.test/context", "name": "context"}
                ]),
            ),
        )),
    )
    .await
    .expect("prompt accepted");
}

#[tokio::test]
async fn node_tap_content_capability_check_accepts_an_advertised_image() {
    prompt_case(
        "capability_check_accepts_image",
        "N4: content capability check (provider_content_commands.rs:318), accepting",
        ProviderSubmission::plain(vec![
            PromptContent::image("image/png".into(), "aGVsbG8=".into(), None).expect("image"),
        ]),
        Some((
            "PromptContents[image]",
            prompt_request(
                GENERATED_REQUEST_ID,
                serde_json::json!([{"type": "image", "mimeType": "image/png", "data": "aGVsbG8="}]),
            ),
        )),
    )
    .await
    .expect("image accepted");
}

#[tokio::test]
async fn node_tap_content_capability_check_refuses_unadvertised_audio_before_the_acp_client() {
    let refusal = prompt_case(
        "capability_check_refuses_audio",
        "N4: content capability check (provider_content_commands.rs:318), refusing",
        ProviderSubmission::plain(vec![
            PromptContent::audio("audio/wav".into(), "aGVsbG8=".into()).expect("audio"),
        ]),
        None,
    )
    .await;
    assert!(matches!(
        refusal,
        Err(CommandFailure::UnsupportedContent("audio"))
    ));
}

#[tokio::test]
async fn node_tap_settings_command_reaches_the_acp_client_as_a_config_option() {
    let rig = command_port_rig(AgentFixture::Behavior {
        agent_case: "settings",
        with_release_file: false,
    })
    .await;
    let mut case = TapCase::new(
        ProtocolNode::N4CommandPort,
        "settings_set_model",
        "N4: Session command (settings) -> set config option to N6",
    );
    let command = SetSessionSettingCommand {
        target: rig.board_target.clone(),
        setting_id: "model".into(),
        value: "new".into(),
        intent: SettingApplicationIntent::ExplicitChange,
        expected_model: None,
        actor: rig.actor.clone(),
    };
    consumed_command(&mut case, "SetSetting", &command);
    rig.port
        .set_setting(command)
        .await
        .expect("setting applied");
    record_agent_wire(&mut case, &rig);
    let row = CatalogRowExpectation::new().produces_with(
        MessageFamily::PromptContents,
        EdgeParty::Node(ProtocolNode::N6AcpClient),
        "session/set_config_option:model=new",
        serde_json::json!({"jsonrpc": "2.0", "id": GENERATED_REQUEST_ID, "method": "session/set_config_option", "params": {"sessionId": "session-1", "configId": "model", "value": "new"}}),
    );
    case.finish(&row).assert_settled();
    rig.supervisor
        .shutdown()
        .await
        .expect("supervisor shutdown");
}

#[tokio::test]
async fn node_tap_cancel_command_reaches_the_acp_client_for_the_held_turn() {
    let rig = command_port_rig(AgentFixture::Behavior {
        agent_case: "settings",
        with_release_file: true,
    })
    .await;
    let held_input = InputId::generate();
    rig.port
        .prompt(PromptSessionCommand {
            target: rig.board_target.clone(),
            input_id: held_input,
            submission: ProviderSubmission::plain(vec![
                PromptContent::text("hold".into()).expect("text"),
            ]),
            actor: rig.actor.clone(),
        })
        .await
        .expect("held prompt started");
    let mut case = TapCase::new(
        ProtocolNode::N4CommandPort,
        "cancel_active_turn",
        "N4: Session command (cancel) -> cancel to N6",
    );
    let command = SessionTargetCommand {
        target: rig.board_target.clone(),
        actor: rig.actor.clone(),
    };
    consumed_command(&mut case, "Cancel", &command);
    rig.port.cancel(command).await.expect("cancel accepted");
    wait_for_turn_end(&rig).await;
    // The held prompt is the first thing the agent received; the cancel follows it.
    record_agent_wire(&mut case, &rig);
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::PromptContents,
            EdgeParty::Node(ProtocolNode::N6AcpClient),
            "PromptContents[text]",
            prompt_request(GENERATED_REQUEST_ID, serde_json::json!([{"type": "text", "text": "hold"}])),
        )
        .produces_with(
            MessageFamily::PromptContents,
            EdgeParty::Node(ProtocolNode::N6AcpClient),
            "session/cancel",
            serde_json::json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": "session-1"}}),
        );
    case.finish(&row).assert_settled();
    rig.supervisor
        .shutdown()
        .await
        .expect("supervisor shutdown");
}

#[tokio::test]
async fn node_tap_steer_command_reaches_the_acp_client_for_the_active_turn() {
    let rig = command_port_rig(AgentFixture::ScriptedSteering).await;
    let first_input = InputId::generate();
    let started = rig
        .port
        .prompt(PromptSessionCommand {
            target: rig.board_target.clone(),
            input_id: first_input,
            submission: ProviderSubmission::plain(vec![
                PromptContent::text("work".into()).expect("text"),
            ]),
            actor: rig.actor.clone(),
        })
        .await
        .expect("prompt started a turn");
    let mut case = TapCase::new(
        ProtocolNode::N4CommandPort,
        "steer_active_turn",
        "N4: Session command (steer) -> steer contents to N6",
    );
    let command = SteerSessionCommand {
        target: rig.board_target.clone(),
        input_id: InputId::new("n4-tap-steer-input").expect("input id"),
        expected_turn_id: started.turn_id.clone(),
        submission: ProviderSubmission::plain(vec![
            PromptContent::text("follow-up".into()).expect("text"),
        ]),
        actor: rig.actor.clone(),
    };
    // The expected turn id is the generated id of the started turn; the sentinel stands for it.
    let mut recorded = command.clone();
    recorded.expected_turn_id = "<volatile:turn-id>".into();
    consumed_command(&mut case, "Steer", &recorded);
    let outcome = rig.port.steer(command).await.expect("steer accepted");
    assert!(matches!(
        outcome,
        SessionSteerOutcome::Injected { turn_id } if turn_id == started.turn_id
    ));
    wait_for_turn_end(&rig).await;
    record_agent_wire(&mut case, &rig);
    let row = CatalogRowExpectation::new().produces_with(
        MessageFamily::PromptContents,
        EdgeParty::Node(ProtocolNode::N6AcpClient),
        "SteerContents[text]",
        serde_json::json!({"jsonrpc": "2.0", "id": GENERATED_REQUEST_ID, "method": "_session/steering", "params": {"sessionId": "fixture-session", "prompt": [{"type": "text", "text": "follow-up"}], "_meta": {"steering": {"idleBehavior": "promptRequired"}}}}),
    );
    case.finish(&row).assert_settled();
    rig.supervisor
        .shutdown()
        .await
        .expect("supervisor shutdown");
}

#[tokio::test]
async fn node_tap_steer_for_a_stale_turn_is_refused_before_the_acp_client() {
    let rig = command_port_rig(AgentFixture::Behavior {
        agent_case: "settings",
        with_release_file: false,
    })
    .await;
    let mut case = TapCase::new(
        ProtocolNode::N4CommandPort,
        "steer_stale_turn_refused",
        "N4: Session command (steer) with no matching active turn -> nothing to N6",
    );
    let command = SteerSessionCommand {
        target: rig.board_target.clone(),
        input_id: InputId::new("n4-tap-stale-input").expect("input id"),
        expected_turn_id: "no-such-turn".into(),
        submission: ProviderSubmission::plain(vec![
            PromptContent::text("late".into()).expect("text"),
        ]),
        actor: rig.actor.clone(),
    };
    consumed_command(&mut case, "Steer", &command);
    let refusal = rig.port.steer(command).await;
    assert!(matches!(refusal, Err(CommandFailure::Busy)));
    record_agent_wire(&mut case, &rig);
    case.finish(&CatalogRowExpectation::new()).assert_settled();
    rig.supervisor
        .shutdown()
        .await
        .expect("supervisor shutdown");
}
