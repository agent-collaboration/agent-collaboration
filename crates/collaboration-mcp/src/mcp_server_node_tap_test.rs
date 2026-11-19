//! N11 node taps: each tool family routes to its target, and the advertised
//! tool schema binds Codex shapes at runtime.
//!
//! Catalog source: the Program Design's N11 section (`N11 --> N4, N8, N10`;
//! "Codex shapes are spliced into tool schemas at runtime
//! (schema_binding.rs:3)") and its producer/consumer table.
//!
//! Stand-ins, each recorded in the report: the Host's provider backend is the
//! seam where a session command leaves N11 today (N4 has no separate in-process
//! port yet); the ACP carrier lands on a fake Codex ACP socket (N8's served
//! surface); native control lands on a fake Codex app-server (N10's peer).
//!
//! Recording layers: each routing case is an MCP caller over the real HTTP/TCP path. Its
//! `initialize`, `tools/list` and `tools/call` requests and the server's whole replies are
//! `WireFrame`s on one caller connection, so each call is checked against the tools that
//! connection advertised; the fake peers' sockets are recorded whole as well (see `wire`). What
//! N11 hands the provider backend is a typed value. The advertised-schema cases ask the
//! `ToolSurface` in-process and stay typed.
use super::ToolSurface;
use crate::api_test_harness::{ServedApi, api_config, test_identity};
use codex_router_test_support::node_taps::{
    CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode, RecordLayer,
    TapCase,
};
use collaboration_contracts::{ProviderConversationBackend, ProviderConversationFuture};
use collaboration_protocol::{
    ConversationCancelRequest, ConversationCloseRequest, ConversationCreateRequest,
    ConversationLoadRequest, ConversationOperationReconcileRequest,
    ConversationOperationShowRequest, ConversationOperationSnapshot,
    ConversationOperationSubmission, ConversationOperationWaitRequest,
    ConversationOperationWaitResult, ConversationPromptRequest, ConversationResumeRequest,
    EndpointRef, ProviderBindingIdentity,
};
use collaboration_runtime::{NativeControlBackend, NativeGenerationGate};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use wire::{
    record_acp_adapter_wire, record_managed_codex_wire, record_mcp_handshake,
    record_tool_call_reply, record_tool_call_request, spawn_fake_acp_adapter, spawn_fake_codex,
};

#[path = "mcp_server_node_tap_wire.rs"]
mod wire;

const SERVICE_ID: &str = "00000000-0000-7000-8000-000000000001";
const SERVICE_EPOCH: &str = "00000000-0000-7000-8000-000000000002";
const CANCEL_OPERATION: &str = "019f0000-0000-7000-8000-000000000006";
const PROMPT_OPERATION: &str = "019f0000-0000-7000-8000-000000000002";

/// The Host's provider backend as a recorder: it names each operation it is
/// asked to run and answers `cancel` with an admitted operation. Anything else
/// is refused, so an unplanned call shows up as a failed tool call.
#[derive(Clone, Default)]
struct RecordingProviderBackend {
    operations: Arc<Mutex<Vec<(&'static str, Value)>>>,
}

impl RecordingProviderBackend {
    fn operations(&self) -> Vec<(&'static str, Value)> {
        self.operations.lock().expect("operations lock").clone()
    }

    fn respond<TResult: serde::de::DeserializeOwned + Send + 'static>(
        &self,
        operation: &'static str,
        request: Value,
        answer: Value,
    ) -> ProviderConversationFuture<'_, TResult> {
        self.operations
            .lock()
            .expect("operations lock")
            .push((operation, request));
        let answered = match answer.get("error") {
            Some(failure) => Err(serde_json::from_value(failure.clone()).expect("failure")),
            None => Ok(serde_json::from_value(answer["result"].clone()).expect("result")),
        };
        Box::pin(async move { answered })
    }

    fn refused(operation: &'static str) -> Value {
        json!({"error":{
            "kind":"protocolViolation","stage":"admission","effect":"none",
            "message":format!("unplanned provider {operation}")
        }})
    }
}

impl ProviderConversationBackend for RecordingProviderBackend {
    fn binding(&self, endpoint: &EndpointRef) -> Option<ProviderBindingIdentity> {
        let binding: ProviderBindingIdentity =
            serde_json::from_value(provider_binding()).expect("provider binding");
        (&binding.endpoint == endpoint).then_some(binding)
    }
    fn create(
        &self,
        request: ConversationCreateRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
        self.respond(
            "create",
            serde_json::to_value(&request).expect("request JSON"),
            Self::refused("create"),
        )
    }
    fn load(
        &self,
        request: ConversationLoadRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
        self.respond(
            "load",
            serde_json::to_value(&request).expect("request JSON"),
            Self::refused("load"),
        )
    }
    fn resume(
        &self,
        request: ConversationResumeRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
        self.respond(
            "resume",
            serde_json::to_value(&request).expect("request JSON"),
            Self::refused("resume"),
        )
    }
    fn close(
        &self,
        request: ConversationCloseRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
        self.respond(
            "close",
            serde_json::to_value(&request).expect("request JSON"),
            Self::refused("close"),
        )
    }
    fn prompt(
        &self,
        request: ConversationPromptRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
        self.respond(
            "prompt",
            serde_json::to_value(&request).expect("request JSON"),
            Self::refused("prompt"),
        )
    }
    fn cancel(
        &self,
        request: ConversationCancelRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
        self.respond(
            "cancel",
            serde_json::to_value(&request).expect("request JSON"),
            json!({"result":{"admission":"admitted","operation":cancel_snapshot()}}),
        )
    }
    fn show(
        &self,
        request: ConversationOperationShowRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSnapshot> {
        self.respond(
            "show",
            serde_json::to_value(&request).expect("request JSON"),
            Self::refused("show"),
        )
    }
    fn wait(
        &self,
        request: ConversationOperationWaitRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationWaitResult> {
        self.respond(
            "wait",
            serde_json::to_value(&request).expect("request JSON"),
            Self::refused("wait"),
        )
    }
    fn reconcile(
        &self,
        request: ConversationOperationReconcileRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSnapshot> {
        self.respond(
            "reconcile",
            serde_json::to_value(&request).expect("request JSON"),
            Self::refused("reconcile"),
        )
    }
}

fn new_case(case: &str, row: &str) -> TapCase {
    TapCase::new(ProtocolNode::N11McpServer, case, row)
}

fn routed_to(
    expectation: CatalogRowExpectation,
    family: MessageFamily,
    node: ProtocolNode,
    kind: &str,
    payload: Value,
) -> CatalogRowExpectation {
    expectation.produces_with(family, EdgeParty::Node(node), kind, payload)
}

fn provider_generation() -> Value {
    json!({"serviceEpoch":SERVICE_EPOCH,"generation":3})
}

/// The binding the scripted backend serves the provider endpoint with.
fn provider_binding() -> Value {
    json!({
        "endpoint":{"serviceId":SERVICE_ID,"endpointId":"claude-code"},
        "bindingId":"binding-1",
        "runtime":{"provider":"claudeCode","runtimeName":"claude-agent-acp","runtimeVersion":"1.0.0"},
        "transport":"stdioAcp",
        "generation":provider_generation(),
        "capabilities":[{"name":"prompt","status":"supported","evidence":"advertised"}]
    })
}

fn provider_endpoint() -> Value {
    json!({
        "endpoint":{"serviceId":SERVICE_ID,"endpointId":"claude-code"},
        "label":"Fixture provider",
        "availability":{"state":"available","observedAt":"2026-09-20T12:00:00Z"},
        "channels":[{
            "kind":"externalProvider","transport":"stdioAcp",
            "bindingId":"binding-1","bindingGeneration":3,
            "runtime":{"provider":"claudeCode","runtimeName":"claude-agent-acp"},
            "capabilities":[{"name":"create","status":"supported","evidence":"advertised"}]
        }]
    })
}

/// A tool call's JSON-RPC answer as the MCP server sends it: the structured result and its JSON
/// text twin.
fn tool_reply(structured: Value, is_error: bool) -> Value {
    json!({"jsonrpc":"2.0","id":2,"result":{
        "content":[{"type":"text","text":structured.to_string()}],
        "structuredContent":structured,
        "isError":is_error
    }})
}

fn cancel_snapshot() -> Value {
    json!({
        "operationId":CANCEL_OPERATION,
        "operation":"conversationCancel",
        "binding":{"kind":"externalProvider","binding":provider_binding()},
        "target":{"endpoint":{"serviceId":SERVICE_ID,"endpointId":"claude-code"},"sessionId":"provider-thread"},
        "stage":"mayHaveDispatched",
        "effect":"unknown",
        "reconciliation":"unresolved",
        "admittedAt":"2026-09-20T12:00:00Z",
        "terminalAt":null
    })
}

#[tokio::test]
async fn node_tap_session_command_tool_routes_to_command_port() {
    let temporary = tempfile::tempdir().expect("temporary service directory");
    let backend = RecordingProviderBackend::default();
    let identity = test_identity()
        .with_endpoints(vec![
            serde_json::from_value(provider_endpoint()).expect("provider endpoint"),
        ])
        .expect("endpoint directory")
        .with_provider_conversation_backend(Arc::new(backend.clone()));
    let listener = ServedApi::tcp(&api_config(identity.build(), temporary.path())).await;
    let endpoint = json!({"serviceId":SERVICE_ID,"endpointId":"claude-code"});
    let target = json!({"endpoint":endpoint,"sessionId":"provider-thread"});
    let actor = json!({"endpoint":endpoint,"sessionId":"caller-session"});
    let mut case = new_case(
        "session_command_tool_to_n4",
        "N11 section: N11 --> N4; table: session command N1, N11 -> N4",
    );
    record_mcp_handshake(&listener, &mut case).await;
    let cancel_arguments = json!({"operationId":CANCEL_OPERATION,"targetOperationId":PROMPT_OPERATION,
        "target":target,"requestedBy":actor,"approver":actor});
    let call = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
        "name":"conversation_cancel",
        "arguments":cancel_arguments
    }});
    record_tool_call_request(&mut case, &call);
    let (_, answer) = listener.post(&call).await;
    assert_eq!(answer["result"]["isError"], false, "{answer}");
    // Today N11 calls the provider supervisor's backend directly, not N4: the listed D2 defect.
    for (operation, request) in backend.operations() {
        case.produced_with(
            RecordLayer::TypedOutcome,
            MessageFamily::SessionCommand,
            EdgeParty::ProviderSupervisor,
            operation,
            request,
        );
    }
    record_tool_call_reply(&mut case, &call, &answer);
    listener.stop().await;
    let cancel_command = json!({"operationId":CANCEL_OPERATION,"targetOperationId":PROMPT_OPERATION,"target":target,"generation":provider_generation(),"requestedBy":actor,"approver":actor});
    let cancel_reply = tool_reply(
        json!({"admission":"admitted","operation":{
            "operationId":CANCEL_OPERATION,
            "operation":"conversationCancel",
            "binding":{"kind":"externalProvider","binding":provider_binding()},
            "target":target,
            "stage":"mayHaveDispatched",
            "effect":"unknown",
            "reconciliation":"unresolved",
            "admittedAt":"2026-09-20T12:00:00Z"
        }}),
        false,
    );
    let routed_through = |command_owner: EdgeParty| {
        CatalogRowExpectation::new()
            .produces_with(
                MessageFamily::SessionCommand,
                command_owner,
                "cancel",
                cancel_command.clone(),
            )
            .produces_with(
                MessageFamily::McpToolCall,
                EdgeParty::McpCaller,
                "tools/call:conversation_cancel",
                cancel_reply.clone(),
            )
    };
    case.current_defect(routed_through(EdgeParty::ProviderSupervisor));
    let row = routed_through(EdgeParty::Node(ProtocolNode::N4CommandPort));
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_acp_carrier_tool_routes_to_codex_acp_adapter() {
    let temporary = tempfile::tempdir().expect("temporary service directory");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private service directory");
    }
    let endpoint = json!({"serviceId":SERVICE_ID,"endpointId":"codex-local"});
    let description = serde_json::from_value(json!({
        "endpoint":endpoint, "label":"Fixture Codex",
        "availability":{"state":"available","observedAt":"2026-09-19T00:00:00Z"},
        "channels":[{"kind":"acp","transport":"unixJsonLines","path":"acp.sock",
            "schemaDigest":format!("sha256:{}", collaboration_protocol::ACP_SCHEMA_DIGEST)}]
    }))
    .expect("endpoint description");
    let identity = test_identity()
        .with_endpoints(vec![description])
        .expect("endpoint directory");
    let acp =
        tokio::net::UnixListener::bind(temporary.path().join("acp.sock")).expect("ACP listener");
    // The fake Codex ACP adapter: answers initialize, keeps its whole transcript, and refuses
    // the load so the carrier stops there.
    let peer = spawn_fake_acp_adapter(acp);
    let listener = ServedApi::tcp(&api_config(identity.build(), temporary.path())).await;
    let mut case = new_case("acp_carrier_tool_to_n8", "N11 section: N11 --ACP--> N8");
    // Router-owned `_meta` is held to the shared owning checks; keys no owning type defines yet
    // are named instead of passed (correction 3, item B).
    case.use_owned_codecs(
        codex_router_test_support::node_taps::OwnedCodecs::from_owner_registry(
            acp_provider_adapters::owned_contract_checks::round_trips(),
            acp_provider_adapters::owned_contract_checks::verdicts(),
        )
        .unwrap_or_else(|error| panic!("owner registry: {error}")),
    );
    case.declare_coverage_gap(
        "session/load",
        "the session/load _meta keys router, codexRouter and codex-router/replayHistory have no owning type: collaboration-client builds them by hand (acp_session_setup.rs:84-91) and N8 reads them by hand (acp_connection_router.rs:62, session_creation.rs:173, history_projection.rs:9)",
    );
    record_mcp_handshake(&listener, &mut case).await;
    let prompt_arguments = json!({
                "target":{"endpoint":endpoint,"sessionId":"resumed-thread"},
                "workingDirectory":temporary.path(),
                "requestedBy":{"endpoint":endpoint,"sessionId":"resumed-thread"},
                "message":{"kind":"humanUser","text":"carrier proof"},"timeoutSeconds":3
    });
    let call = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
        "name":"conversation_prompt","arguments":prompt_arguments
    }});
    record_tool_call_request(&mut case, &call);
    let (_, answer) = listener.post(&call).await;
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    listener.stop().await;
    let transcript = peer.await.expect("ACP peer join");
    // Table gap: N11's ACP calls toward N8 have no row (the table only has N8's
    // served ACP); recorded under N8's served-ACP row with N11 as its client.
    record_acp_adapter_wire(&mut case, &transcript);
    record_tool_call_reply(&mut case, &call, &answer);
    let row = routed_to(
        routed_to(
            CatalogRowExpectation::new(),
            MessageFamily::AcpRequestToCodexAdapter,
            ProtocolNode::N8CodexAcpAdapter,
            "initialize",
            json!({"jsonrpc":"2.0","id":"<volatile:id>","method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"agent-collaboration","version":"<volatile:version>"}}}),
        ),
        MessageFamily::AcpRequestToCodexAdapter,
        ProtocolNode::N8CodexAcpAdapter,
        "session/load",
        json!({"jsonrpc":"2.0","id":"<volatile:id>","method":"session/load","params":{"sessionId":"resumed-thread","cwd":temporary.path(),"mcpServers":[],"_meta":{"codexRouter":{},"router":{"sessionRef":{"endpoint":endpoint,"sessionId":"resumed-thread"}},"codex-router/replayHistory":false}}}),
    )
    .produces_with(
        MessageFamily::McpToolCall,
        EdgeParty::McpCaller,
        "tools/call:conversation_prompt",
        tool_reply(
            json!({
                "mcpResult":"error","kind":"rejected",
                "message":"collaboration API request rejected with code -32050",
                "stage":"load","effect":"unknown",
                "target":{"endpoint":endpoint,"sessionId":"resumed-thread"},
                "turnId":null,"serviceKind":"nativeRejected",
                "data":{"kind":"nativeRejected","stage":"load","reason":"fixture-policy"},
                "code":-32050
            }),
            true,
        ),
    );
    case.finish(&row).assert_settled();
}

fn native_message_schemas() -> (Arc<codex_native_integration::NativePayloadSchemas>, String) {
    let mut definitions = BTreeMap::new();
    for operation in [
        "ThreadRead",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
        "ThreadQueueAdd",
        "ThreadNameSet",
    ] {
        definitions.insert(format!("{operation}Params"), json!({"type":"object"}));
        definitions.insert(format!("{operation}Response"), json!({"type":"object"}));
    }
    let bundle = codex_native_integration::NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}})).expect("schema JSON"),
    )]))
    .expect("native schema bundle");
    let schemas = Arc::new(
        codex_native_integration::NativePayloadSchemas::from_bundle(&bundle)
            .expect("native payload schemas"),
    );
    let digest = schemas.schema_digest().to_owned();
    (schemas, digest)
}

#[tokio::test]
async fn node_tap_native_control_tool_routes_to_codex_client() {
    let directory = tempfile::tempdir_in("/tmp").expect("private service directory");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private permissions");
    }
    let endpoint_value = json!({"serviceId":SERVICE_ID,"endpointId":"codex-local"});
    let generation: collaboration_protocol::CodexGeneration =
        serde_json::from_value(json!({"serviceEpoch":SERVICE_EPOCH,"generation":7}))
            .expect("generation");
    let (schemas, digest) = native_message_schemas();
    let backend_path = directory.path().join("native.sock");
    let native_listener = tokio::net::UnixListener::bind(&backend_path).expect("native bind");
    let gate = NativeGenerationGate::default();
    gate.activate(generation.clone(), backend_path, Some(schemas))
        .expect("activate generation");
    let description: collaboration_protocol::EndpointDescription = serde_json::from_value(json!({
        "endpoint":endpoint_value, "label":"Native fixture",
        "availability":{"state":"available","observedAt":"2026-09-19T00:00:00Z"},
        "channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"native.sock",
            "schemaDigest":digest,"generation":generation}]
    }))
    .expect("endpoint description");
    let backend = NativeControlBackend {
        endpoint: serde_json::from_value(endpoint_value.clone()).expect("endpoint ref"),
        gate,
        codex_home: directory.path().to_owned(),
    };
    let identity = test_identity()
        .with_endpoints(vec![description])
        .expect("endpoint directory")
        .with_native_backend(backend)
        .expect("native backend");
    // The fake Codex app-server answers each request and keeps its whole transcript.
    let codex = spawn_fake_codex(native_listener);
    let listener = ServedApi::tcp(&api_config(identity.build(), directory.path())).await;
    let mut case = new_case(
        "native_control_tool_to_n10",
        "N11 section: N11 --> N10; table: Codex request (to Codex) from N10",
    );
    record_mcp_handshake(&listener, &mut case).await;
    let interrupt_arguments = json!({
                    "target":{"endpoint":endpoint_value,"sessionId":"thread-a"},
                    "generation":generation,"turnId":"turn-a"});
    let call = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
        "name":"turn_interrupt","arguments":interrupt_arguments
    }});
    record_tool_call_request(&mut case, &call);
    let (_, answer) = listener.post(&call).await;
    assert_eq!(answer["result"]["isError"], false, "{answer}");
    listener.stop().await;
    let transcript = codex.await.expect("native fixture join");
    // The whole socket: N10's own connect handshake (`initialize`, `initialized`) is recorded and
    // checked; the call N11 routes is the operation after it.
    record_managed_codex_wire(&mut case, &transcript);
    record_tool_call_reply(&mut case, &call, &answer);
    let row = routed_to(
        CatalogRowExpectation::new(),
        MessageFamily::CodexRequestToCodex,
        ProtocolNode::N10CodexClient,
        "turn/interrupt",
        json!({"id":"agent-collaboration-1","method":"turn/interrupt","params":{"threadId":"thread-a","turnId":"turn-a"}}),
    )
    .produces_with(
        MessageFamily::McpToolCall,
        EdgeParty::McpCaller,
        "tools/call:turn_interrupt",
        tool_reply(
            json!({
                "target":{"endpoint":endpoint_value,"sessionId":"thread-a"},
                "generation":generation,
                "turnId":"turn-a","kind":"interruptCompleted"
            }),
            false,
        ),
    );
    case.finish(&row).assert_settled();
}

/// The whole `session_inspect` tool advertised without Codex definitions: the baseline recorded from
/// today's surface, kept as a file so the whole value is compared (name, description, input and
/// output schema).
fn opaque_session_inspect_tool() -> Value {
    serde_json::from_str(include_str!(
        "mcp_server_node_tap_session_inspect_tool.json"
    ))
    .expect("baseline tool JSON")
}

/// The same tool once the Codex `Thread` definition exists: its `thread` property points at the
/// bound definition and the schema carries that definition (the two edits schema binding makes).
fn bound_session_inspect_tool() -> Value {
    let mut tool = opaque_session_inspect_tool();
    tool["outputSchema"]["$defs"]["NativeInspectResult"]["properties"]["thread"] =
        json!({"$ref":"#/$defs/codexNativeThread"});
    tool["outputSchema"]["$defs"]["codexNativeThread"] =
        json!({"type":"object","required":["id"],"properties":{"id":{"type":"string"}}});
    tool
}

fn thread_definitions() -> serde_json::Map<String, Value> {
    serde_json::Map::from_iter([
        (
            "Thread".to_owned(),
            json!({"type":"object","required":["id"],"properties":{"id":{"type":"string"}}}),
        ),
        ("ThreadStatus".to_owned(), json!({"type":"string"})),
    ])
}

/// What an advertised tool schema says about Codex shapes: bound definitions
/// the schema carries, and whether any `urn:codex-native` reference survives.
fn schema_kind(surface: &ToolSurface, tool_name: &str) -> (String, Value) {
    let tool = surface
        .tools()
        .iter()
        .find(|tool| tool.name == tool_name)
        .unwrap_or_else(|| panic!("tool {tool_name}"));
    let schema = serde_json::to_value(&tool.output_schema).expect("schema JSON");
    let output = schema.to_string();
    // Bound means the schema carries the definition and points its `$ref` at it.
    let bound = schema.pointer("/$defs/codexNativeThread").is_some()
        && output.contains("#/$defs/codexNativeThread\"");
    let unresolved = output.contains("urn:codex-native");
    (
        format!("tool_schema:{tool_name} codexNativeThread={bound} unresolvedRefs={unresolved}"),
        serde_json::to_value(tool).expect("tool JSON"),
    )
}

#[test]
fn node_tap_advertised_schema_binds_codex_shapes_when_definitions_exist() {
    let definitions = thread_definitions();
    let surface = ToolSurface::new(Some(
        &crate::NativeSchemaDefinitions::from_bundle(&bundle_with(&definitions))
            .expect("bundle definitions"),
    ));
    let mut case = new_case(
        "advertised_schema_bound_with_codex_definitions",
        "N11 section: Codex shapes spliced into tool schemas at runtime (schema_binding.rs:3)",
    );
    case.state_before(NodeStateSnapshot::new().field("native_definitions", "available"));
    // The surface is asked in-process, not over the wire: both records are typed values. The
    // served `tools/list` result is recorded whole, at the wire, by the routing cases.
    case.consumed(
        RecordLayer::TypedInput,
        MessageFamily::McpToolCall,
        EdgeParty::McpCaller,
        "tools/list",
    );
    let (kind, payload) = schema_kind(&surface, "session_inspect");
    case.produced_with(
        RecordLayer::TypedOutcome,
        MessageFamily::McpToolSchema,
        EdgeParty::McpCaller,
        &kind,
        payload,
    );
    case.state_after(NodeStateSnapshot::new().field("native_definitions", "available"));
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::McpToolSchema,
            EdgeParty::McpCaller,
            "tool_schema:session_inspect codexNativeThread=true unresolvedRefs=false",
            bound_session_inspect_tool(),
        )
        .state_after(NodeStateSnapshot::new().field("native_definitions", "available"));
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_advertised_schema_is_opaque_without_codex_definitions() {
    let surface = ToolSurface::new(None);
    let mut case = new_case(
        "advertised_schema_opaque_without_codex_definitions",
        "N11 section: Codex shapes spliced into tool schemas at runtime (schema_binding.rs:3)",
    );
    case.state_before(NodeStateSnapshot::new().field("native_definitions", "none"));
    // The surface is asked in-process, not over the wire: both records are typed values. The
    // served `tools/list` result is recorded whole, at the wire, by the routing cases.
    case.consumed(
        RecordLayer::TypedInput,
        MessageFamily::McpToolCall,
        EdgeParty::McpCaller,
        "tools/list",
    );
    let (kind, payload) = schema_kind(&surface, "session_inspect");
    case.produced_with(
        RecordLayer::TypedOutcome,
        MessageFamily::McpToolSchema,
        EdgeParty::McpCaller,
        &kind,
        payload,
    );
    case.state_after(NodeStateSnapshot::new().field("native_definitions", "none"));
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::McpToolSchema,
            EdgeParty::McpCaller,
            "tool_schema:session_inspect codexNativeThread=false unresolvedRefs=false",
            opaque_session_inspect_tool(),
        )
        .state_after(NodeStateSnapshot::new().field("native_definitions", "none"));
    case.finish(&row).assert_settled();
}

fn bundle_with(
    definitions: &serde_json::Map<String, Value>,
) -> codex_native_integration::NativeSchemaBundle {
    codex_native_integration::NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}})).expect("schema JSON"),
    )]))
    .expect("native schema bundle")
}
