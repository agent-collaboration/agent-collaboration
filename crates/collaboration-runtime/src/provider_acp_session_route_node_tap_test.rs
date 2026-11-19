//! N9 node taps: the provider ACP route, driven in-process over a duplex pipe
//! through the same transport the production listener uses, with the real
//! provider Session event hub behind it and a recording command port at N4.
use std::sync::{Arc, Mutex};

use codex_acp_adapter::{
    AcpConnectionContext, AcpSessionRoute, acp_connection_channels, read_acp_frame,
    run_acp_transport, write_acp_frame,
};
use codex_router_test_support::node_taps::{
    CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode, RecordLayer,
    TapCase,
};
use collaboration_contracts::{
    CommandFailure, CommandFuture, CreateSessionCommand, PromptSessionCommand, QueueInputCommand,
    QueuedSessionInput, SessionCommandPort, SessionEventHub, SessionSteerOutcome,
    SessionTargetCommand, SessionTurnHandle, SetSessionSettingCommand, SteerSessionCommand,
};
use message_board::{Identity, SessionRef};
use serde_json::{Value, json};
use session_event_model::{
    CapabilityReport, SessionEvent, SessionItem, SessionItemKind, SessionState, StopReason,
    TurnOutcome,
};
use tokio::{
    io::{BufReader, ReadHalf, WriteHalf},
    sync::Mutex as AsyncMutex,
};

use super::ProviderAcpSessionRoute;
use crate::{ProviderOperationStore, ProviderSessionEventHub};

const N9: ProtocolNode = ProtocolNode::N9ProviderAcpRoute;
const ACP_CLIENT: EdgeParty = EdgeParty::McpCaller;
const N4: EdgeParty = EdgeParty::Node(ProtocolNode::N4CommandPort);
const N5: EdgeParty = EdgeParty::Node(ProtocolNode::N5EventHub);

/// Generated per request (a fresh UUIDv7), so the recording replaces only this field with a fixed
/// sentinel; every other field of the command is recorded and compared.
const VOLATILE_INPUT_ID: &str = "<volatile:input-id>";

fn volatile_input_id() -> session_event_model::InputId {
    session_event_model::InputId::new(VOLATILE_INPUT_ID).expect("sentinel input id")
}

/// The whole command as its `Debug` text (the command types have no serde).
fn whole_command(command: &impl std::fmt::Debug) -> Value {
    json!({"debug": format!("{command:#?}")})
}

fn literal_actor() -> Identity {
    serde_json::from_value(json!({"kind":"human","humanId":"owner"})).expect("actor")
}

/// Records every command N9 sends to N4 and answers the minimum.
struct RecordingCommandPort {
    session: SessionRef,
    seen: Mutex<Vec<(String, Value)>>,
}

impl RecordingCommandPort {
    fn note(&self, command: &str, detail: Value) {
        self.seen
            .lock()
            .expect("lock")
            .push((command.to_owned(), detail));
    }
    fn commands(&self) -> Vec<(String, Value)> {
        self.seen.lock().expect("lock").clone()
    }
}

impl SessionCommandPort for RecordingCommandPort {
    fn create(&self, command: CreateSessionCommand) -> CommandFuture<'_, SessionRef> {
        self.note("Create", whole_command(&command));
        let session = self.session.clone();
        Box::pin(async move { Ok(session) })
    }
    fn load_session(&self, command: SessionTargetCommand) -> CommandFuture<'_, ()> {
        self.note("Load", whole_command(&command));
        Box::pin(async { Ok(()) })
    }
    fn resume_session(&self, command: SessionTargetCommand) -> CommandFuture<'_, ()> {
        self.note("Resume", whole_command(&command));
        Box::pin(async { Ok(()) })
    }
    fn prompt(&self, command: PromptSessionCommand) -> CommandFuture<'_, SessionTurnHandle> {
        let mut command = command;
        command.input_id = volatile_input_id();
        self.note("Prompt", whole_command(&command));
        Box::pin(async {
            Ok(SessionTurnHandle {
                turn_id: "turn-1".into(),
            })
        })
    }
    fn compact(&self, _: SessionTargetCommand) -> CommandFuture<'_, SessionTurnHandle> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn steer(&self, _: SteerSessionCommand) -> CommandFuture<'_, SessionSteerOutcome> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn queue_add(&self, _: QueueInputCommand) -> CommandFuture<'_, QueuedSessionInput> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn queue_list(&self, _: SessionTargetCommand) -> CommandFuture<'_, Vec<QueuedSessionInput>> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn queue_cancel(&self, _: SessionTargetCommand, _: String) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn cancel(&self, command: SessionTargetCommand) -> CommandFuture<'_, ()> {
        self.note("Interrupt", whole_command(&command));
        Box::pin(async { Ok(()) })
    }
    fn close(&self, _: SessionTargetCommand) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn set_setting(&self, _: SetSessionSettingCommand) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
}

struct RouteHarness {
    hub: Arc<ProviderSessionEventHub>,
    commands: Arc<RecordingCommandPort>,
    session: SessionRef,
    write: WriteHalf<tokio::io::DuplexStream>,
    read: BufReader<ReadHalf<tokio::io::DuplexStream>>,
    /// Every frame on the duplex connection, in the order the test wrote or read it.
    transcript: Vec<WireCrossing>,
    /// How many transcript frames a case has already recorded.
    recorded: usize,
    _root: tempfile::TempDir,
}

/// One frame on the ACP connection, as it crossed, from N9's side.
enum WireCrossing {
    ToRoute(Value),
    FromRoute(Value),
}

impl RouteHarness {
    async fn start() -> Self {
        let root = tempfile::tempdir().expect("root");
        let mut store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store");
        let session: SessionRef = serde_json::from_value(json!({
            "endpoint":{"serviceId":uuid::Uuid::now_v7(),"endpointId":"claude-local"},
            "sessionId":uuid::Uuid::now_v7()
        }))
        .expect("session");
        let owner: collaboration_protocol::ProviderIdentity =
            serde_json::from_value(json!({"humanId":"owner"})).expect("owner");
        store
            .record_session(&crate::ProviderSessionRecord {
                target: serde_json::from_value(serde_json::to_value(&session).expect("target"))
                    .expect("target"),
                working_directory: root.path().display().to_string().try_into().expect("cwd"),
                requested_policy: collaboration_protocol::ProviderRequestedPolicy {
                    access: collaboration_protocol::RouterAccess::WriteRestricted,
                },
                created_by: owner.clone(),
                approver: owner,
                updated_at_ms: 1,
                resume: Default::default(),
            })
            .await
            .expect("inventory");
        let hub = Arc::new(ProviderSessionEventHub::new(Arc::new(AsyncMutex::new(
            store,
        ))));
        for event in [
            SessionEvent::CapabilitiesChanged {
                capabilities: CapabilityReport::default(),
            },
            SessionEvent::StateChanged {
                state: SessionState::Idle,
            },
        ] {
            hub.publish(session.clone(), event).await.expect("seed");
        }
        let commands = Arc::new(RecordingCommandPort {
            session: session.clone(),
            seen: Mutex::new(Vec::new()),
        });
        let (wire, router) = acp_connection_channels();
        let (client, server) = tokio::io::duplex(1 << 20);
        tokio::spawn(run_acp_transport(server, wire));
        let actor: Identity =
            serde_json::from_value(json!({"kind":"human","humanId":"owner"})).expect("actor");
        let route = ProviderAcpSessionRoute::new(
            session.endpoint.clone(),
            Arc::clone(&commands) as Arc<dyn SessionCommandPort>,
            Arc::clone(&hub) as Arc<dyn SessionEventHub>,
        );
        tokio::spawn(Box::new(route).run(
            router,
            AcpConnectionContext {
                actor: Some(actor),
                client_profile: None,
                client_supports_elicitation_form: false,
            },
        ));
        let (read, write) = tokio::io::split(client);
        Self {
            hub,
            commands,
            session,
            write,
            read: BufReader::new(read),
            transcript: Vec::new(),
            recorded: 0,
            _root: root,
        }
    }

    /// Writes one request frame and keeps the exact frame written in the transcript.
    async fn send(&mut self, id: u64, method: &str, params: Value) {
        let frame = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        write_acp_frame(&mut self.write, &frame)
            .await
            .expect("write frame");
        self.transcript.push(WireCrossing::ToRoute(frame));
    }

    async fn next_frame(&mut self) -> Value {
        let frame = read_acp_frame(&mut self.read)
            .await
            .expect("read frame")
            .expect("frame before close");
        self.transcript.push(WireCrossing::FromRoute(frame.clone()));
        frame
    }

    /// Sends a request the route answers with method-not-found and reads every frame the route
    /// wrote before that answer, so a duplicated or undeclared frame becomes an extra record.
    async fn drain_until_marker(&mut self) -> Vec<Value> {
        self.send(MARKER_ID, MARKER_METHOD, json!({})).await;
        let mut frames = Vec::new();
        loop {
            let frame = self.next_frame().await;
            if frame.get("method").is_none() && frame.get("id") == Some(&json!(MARKER_ID)) {
                return frames;
            }
            frames.push(frame);
        }
    }

    /// Reads frames up to and including the response to request `id`, then every frame written
    /// before the marker answer.
    async fn drain_through_response(&mut self, id: u64) -> Vec<Value> {
        let mut frames = Vec::new();
        loop {
            let frame = self.next_frame().await;
            let is_response = frame.get("method").is_none() && frame.get("id") == Some(&json!(id));
            frames.push(frame);
            if is_response {
                break;
            }
        }
        frames.extend(self.drain_until_marker().await);
        frames
    }

    async fn open_session(&mut self) {
        self.send(1, "session/new", json!({"cwd":"/tmp","mcpServers":[]}))
            .await;
        let frames = self.drain_until_marker().await;
        assert!(
            frames.iter().any(|frame| frame.get("result").is_some()),
            "{frames:?}"
        );
    }
}

const MARKER_ID: u64 = 900;
/// A method N9 does not serve: the harness's end-of-output marker, a test stand-in.
const MARKER_METHOD: &str = "unknown/marker";

#[path = "provider_acp_session_route_node_tap_recording_test.rs"]
mod recording;
use recording::{Segment, record_connection, register_router_codecs};

fn commands_state(harness: &RouteHarness) -> NodeStateSnapshot {
    NodeStateSnapshot::new().field("session_commands_issued", harness.commands.commands().len())
}

/// A case on `harness`'s connection, held to the Router-owned codecs N9 serves.
fn route_case(case_name: &str, catalog_row: &str) -> TapCase {
    let mut case = TapCase::new(N9, case_name, catalog_row);
    register_router_codecs(&mut case);
    case
}

/// N9's exact answer to the harness's `session/new` request `id`.
fn session_new_response(harness: &RouteHarness, id: u64) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":{
        "sessionId":harness.session.session_id.as_str(),
        "_meta":{"router":{
            "sessionRef":{
                "endpoint":{
                    "serviceId":harness.session.endpoint.service_id.as_str(),
                    "endpointId":"claude-local"
                },
                "sessionId":harness.session.session_id.as_str()
            },
            "approver":{"kind":"human","humanId":"owner"}
        },
        "sessionProfile":{"capabilities":{
            "load":false,"resume":false,"close":false,"list":false,"steer":false,"queue":null,
            "modes":false,"configOptions":false,"elicitation":false,"usage":false,
            "promptContent":{"image":false,"audio":false,"embeddedContext":false},
            "authStatus":{"kind":"notReported"}
        },"historyUnavailable":false}}
    }})
}

fn create_command(harness: &RouteHarness) -> Value {
    whole_command(&CreateSessionCommand {
        endpoint: harness.session.endpoint.clone(),
        working_directory: "/tmp".into(),
        // D1 R1: a mode-less ACP session/new asks for the Router's auto by name.
        settings: collaboration_contracts::SessionSettingsCommand {
            mode: Some("auto".into()),
            ..collaboration_contracts::SessionSettingsCommand::default()
        },
        actor: literal_actor(),
    })
}

#[tokio::test]
async fn node_tap_n9_session_new_request_creates_a_session_through_n4() {
    // Arrange
    let mut harness = RouteHarness::start().await;
    let mut case = route_case(
        "session_new_request",
        "N9 node: ACP client request -> session command -> response",
    );
    case.state_before(commands_state(&harness));
    // Act
    harness
        .send(1, "session/new", json!({"cwd":"/tmp","mcpServers":[]}))
        .await;
    harness.drain_until_marker().await;
    record_connection(&mut case, &mut harness, 0, Segment::CaseExchange);
    case.state_after(commands_state(&harness));
    // Assert
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::SessionCommand,
            N4,
            "Create",
            create_command(&harness),
        )
        .produces_with(
            MessageFamily::AcpServedForRouterSessions,
            ACP_CLIENT,
            "response:session/new",
            session_new_response(&harness, 1),
        )
        .state_after(NodeStateSnapshot::new().field("session_commands_issued", 1));
    case.finish(&row).assert_settled();
}

/// A case whose connection was set up by [`RouteHarness::open_session`]: those frames head the
/// case's capture as fixture setup, checked but not compared with the row.
async fn opened_route_case(case_name: &str, catalog_row: &str) -> (TapCase, RouteHarness) {
    let mut harness = RouteHarness::start().await;
    harness.open_session().await;
    let mut case = route_case(case_name, catalog_row);
    record_connection(&mut case, &mut harness, 0, Segment::Setup);
    case.state_before(commands_state(&harness));
    (case, harness)
}

#[tokio::test]
async fn node_tap_n9_session_prompt_request_sends_prompt_and_answers_with_stop_reason() {
    // Arrange
    let (mut case, mut harness) = opened_route_case(
        "session_prompt_request",
        "N9 node: session/prompt -> Prompt command; TurnEnded -> stop reason response",
    )
    .await;
    let session_id = harness.session.session_id.as_str().to_owned();
    let turn_ended = SessionEvent::TurnEnded {
        turn_id: "turn-1".into(),
        outcome: TurnOutcome::Ended {
            stop_reason: StopReason::EndTurn,
            local_cause: None,
        },
    };
    case.consumed_with(
        RecordLayer::TypedInput,
        MessageFamily::SessionEvent,
        N5,
        "TurnEnded",
        serde_json::to_value(&turn_ended).expect("encode session event"),
    );
    // Act
    harness
        .send(
            2,
            "session/prompt",
            json!({"sessionId":session_id,"prompt":[{"type":"text","text":"hello"}]}),
        )
        .await;
    harness
        .hub
        .publish(harness.session.clone(), turn_ended)
        .await
        .expect("publish");
    harness.drain_through_response(2).await;
    record_connection(&mut case, &mut harness, 1, Segment::CaseExchange);
    case.state_after(commands_state(&harness));
    // Assert
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::SessionCommand,
            N4,
            "Prompt",
            whole_command(&PromptSessionCommand {
                target: harness.session.clone(),
                input_id: volatile_input_id(),
                submission: session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::Text {
                        text: session_event_model::PromptText::try_new("hello".to_owned())
                            .expect("text"),
                    },
                ]),
                actor: literal_actor(),
            }),
        )
        .produces_with(
            MessageFamily::AcpServedForRouterSessions,
            ACP_CLIENT,
            "response:session/prompt",
            json!({"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}}),
        )
        .state_after(NodeStateSnapshot::new().field("session_commands_issued", 2));
    case.finish(&row).assert_settled();
}

async fn tap_session_update(
    case_name: &str,
    row: &str,
    item: SessionItem,
) -> (TapCase, RouteHarness) {
    // Arrange: an open session observed by the route.
    let (mut case, mut harness) = opened_route_case(case_name, row).await;
    let item_label = match &item.kind {
        SessionItemKind::AgentMessage => "ItemStarted(AgentMessage)",
        SessionItemKind::Usage => "ItemStarted(Usage payload=jsonText)",
        _ => "ItemStarted",
    };
    case.consumed_with(
        RecordLayer::TypedInput,
        MessageFamily::SessionEvent,
        N5,
        item_label,
        serde_json::to_value(SessionEvent::ItemStarted { item: item.clone() })
            .expect("encode session event"),
    );
    // Act
    harness
        .hub
        .publish(harness.session.clone(), SessionEvent::ItemStarted { item })
        .await
        .expect("publish");
    harness.drain_until_marker().await;
    record_connection(&mut case, &mut harness, 1, Segment::CaseExchange);
    case.state_after(commands_state(&harness));
    (case, harness)
}

#[tokio::test]
async fn node_tap_n9_agent_message_event_is_rendered_as_session_update() {
    // Act
    let (case, harness) = tap_session_update(
        "agent_message_session_update",
        "N9 node: session event rendered as ACP session/update",
        SessionItem {
            item_id: "item-1".into(),
            kind: SessionItemKind::AgentMessage,
            text: Some("hello".into()),
        },
    )
    .await;
    // Assert
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::AcpServedForRouterSessions,
            ACP_CLIENT,
            "session/update:agent_message_chunk",
            json!({"jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":harness.session.session_id.as_str(),
                "update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hello"}}
            }}),
        )
        .state_after(NodeStateSnapshot::new().field("session_commands_issued", 1));
    case.finish(&row).assert_settled();
}

/// N9 section: N9 shares N3's interpretation, including the metadata filter
/// (D6). Today N9 renders a Usage item as an agent message chunk whose text is
/// the JSON string.
#[tokio::test]
async fn node_tap_n9_usage_metadata_item_must_not_render_as_agent_text() {
    // Act
    let (mut case, harness) = tap_session_update(
        "usage_item_rendered_as_agent_text",
        "N9 node: shares N3's metadata filter (ProviderMetadataNotTranscript)",
        SessionItem {
            item_id: "usage-1".into(),
            kind: SessionItemKind::Usage,
            text: Some(r#"{"used":120,"size":1000}"#.into()),
        },
    )
    .await;
    // Assert: target is no transcript update; today's wire is an agent chunk carrying the JSON text.
    case.current_defect(
        CatalogRowExpectation::new()
            .produces_with(
                MessageFamily::AcpServedForRouterSessions,
                ACP_CLIENT,
                "session/update:agent_message_chunk",
                json!({"jsonrpc":"2.0","method":"session/update","params":{
                    "sessionId":harness.session.session_id.as_str(),
                    "update":{"sessionUpdate":"agent_message_chunk","content":{
                        "type":"text","text":"{\"used\":120,\"size\":1000}"}}
                }}),
            )
            .state_after(NodeStateSnapshot::new().field("session_commands_issued", 1)),
    );
    let row = CatalogRowExpectation::new()
        .state_after(NodeStateSnapshot::new().field("session_commands_issued", 1));
    case.finish(&row).assert_settled();
}
