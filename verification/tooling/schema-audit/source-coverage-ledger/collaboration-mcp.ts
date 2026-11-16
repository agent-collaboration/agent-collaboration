import type { LedgerSection, Owner, SendSite, SendSiteCoverage } from "../schema-audit-types.ts";
// Left out by name only: 28 colocated unit-test files (*_test.rs, *_tests.rs).

/**
 * Send-site rule for this crate (R51, literal site-per-send): every MCP tool reply is a send on
 * the mcp edge. A `#[tool]` fn is one site at its `#[tool]` attribute line. A tool registered
 * through a macro (`domain_tool!`, `board_tool!`, `conversation_operation_tool!`) is one site at
 * its registration line; the reply is written by the macro body named in `sends`. Replies the
 * server builds outside a tool (initialize, tools/list, early call endings, admission shedding)
 * are their own sites.
 */
const N11: Owner = { part: "N11" };
const MCP_SERVER = "crates/collaboration-mcp/src/mcp_server";
const N11_TAP = "crates/collaboration-mcp/src/mcp_server_node_tap_test.rs";
/** Every routing case records the MCP handshake at the WireFrame layer, marked setup. */
const HANDSHAKE_CASES = [
  "session_command_tool_to_n4",
  "acp_carrier_tool_to_n8",
  "native_control_tool_to_n10",
];

function notTapped(tool: string): SendSiteCoverage {
  return {
    kind: "uncovered",
    reason: `The N11 node tap (mcp_server_node_tap_test.rs) records tools/call replies at the WireFrame layer only for conversation_cancel, conversation_prompt and turn_interrupt; no case calls ${tool}.`,
  };
}

/** A `#[tool]` fn's reply. */
function toolReply(
  file: string,
  line: number,
  tool: string,
  coverage: SendSiteCoverage = notTapped(tool),
): SendSite {
  return {
    file: `${MCP_SERVER}/${file}`,
    line,
    edge: "mcp",
    sends: `tools/call reply for ${tool}`,
    owner: N11,
    origin: "product",
    coverage,
  };
}

const MACRO_BODIES: Record<string, string> = {
  domain_tool: "application_tool! body catalog_tools.rs:36 (decode refusal) and :42 (result)",
  board_tool: "board_tool! body catalog_tools.rs:88 (classified refusal) and :95 (result)",
  conversation_operation_tool:
    "conversation_operation_tool! body conversation_operation_tools.rs:27 (decode refusal), :35 (caller cancelled) and :39 (result)",
};

/** A macro-registered tool's reply, at its registration line. */
function macroToolReply(file: string, line: number, tool: string, macro: string): SendSite {
  return {
    file: `${MCP_SERVER}/${file}`,
    line,
    edge: "mcp",
    sends: `tools/call reply for ${tool}, written by the ${MACRO_BODIES[macro]}`,
    owner: N11,
    origin: "product",
    coverage: notTapped(tool),
  };
}

function notCalled(sends: string): SendSiteCoverage {
  return {
    kind: "uncovered",
    reason: `No N11 node tap case records this frame at the WireFrame layer: the ACP adapter connection records only initialize and session/load (mcp_server_node_tap_wire.rs:198-219, acp_carrier_tool_to_n8) and the managed-codex connection only N10's handshake and turn/interrupt (wire.rs:279-303, native_control_tool_to_n10); ${sends}.`,
  };
}

/** The ACP adapter frame acp_carrier_tool_to_n8 records at WireFrame (wire.rs:198). */
const ACP_ADAPTER_TAP: SendSiteCoverage = {
  kind: "tap",
  testFile: N11_TAP,
  cases: ["acp_carrier_tool_to_n8"],
};

/** A call in this crate into a library sender that writes on an outside edge. */
function callerSite(
  file: string,
  line: number,
  edge: SendSite["edge"],
  sends: string,
  coverage: SendSiteCoverage = notCalled("not this caller"),
): SendSite {
  return {
    file: `crates/collaboration-mcp/src/${file}`,
    line,
    edge,
    sends,
    owner: N11,
    origin: "product",
    coverage,
  };
}

export const section: LedgerSection = {
  root: "crates/collaboration-mcp",
  files: [
    {
      file: "crates/collaboration-mcp/src/api_test_harness.rs",
      disposition: {
        kind: "excluded",
        reason: "cfg(test)-only module: declared at lib.rs:24 under #[cfg(test)] (lib.rs:23)",
      },
    },
    {
      file: "crates/collaboration-mcp/src/collaboration_api_router.rs",
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "Serves the MCP streamable-HTTP endpoint (COLLABORATION_API_PATH L29, json responses L102). shed_past_capacity L236 decodes a shed request body as rmcp ClientJsonRpcMessage L254 to answer it with its id: a tools/call gets overloaded_tool_result L279 (json! L280 kind overloaded) L257, any other request the JSON-RPC error overloaded_error L288 (data json! L293) L261; both written by json_message_response L263/L298.",
    },
    {
      file: "crates/collaboration-mcp/src/collaboration_api_router_tests/call_lifecycle.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared by #[path] at collaboration_api_router_tests.rs:569-570, itself a #[cfg(test)] module at lib.rs:31-32",
      },
    },
    {
      file: "crates/collaboration-mcp/src/lib.rs",
      disposition: { kind: "none" },
      note: "Module list and re-exports; test modules under #[cfg(test)] L23-L55; test_support behind the test-support feature L20-L21.",
    },
    {
      file: "crates/collaboration-mcp/src/local_collaboration.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "In-process LocalCollaboration adapter for the carrier tools (L35). rejected L27 converts each application rejection's PublishedRejection into ClientError::Rejected {code, data}, the shape a caller of the API would see, for every provider operation L40-L123.",
    },
    {
      file: "crates/collaboration-mcp/src/loopback_bind_address.rs",
      disposition: { kind: "none" },
      note: "Loopback listener address validation (LoopbackBindAddress L6); no message shapes.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server.rs",
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "ToolSurface::new L98 registers every tool router (the MCP tool-name table); resolve_tool_schemas L195 post-processes advertised schemas (schema_binding.rs, normalize_boolean_json_schemas L234). ServerHandler: initialize L305 answers with router_build_warning instructions L313; call_tool L319 answers early endings L342 (caller_cancelled) and L345 (call_abandoned_at_shutdown) and rewrites rmcp text-only refusals via structured_argument_refusal L349; list_tools L357 answers tools/list.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/application_tool_results.rs",
      disposition: { kind: "converts", owners: ["mcp", "router-vocabulary"] },
      note: "published_failure L24 turns a PublishedRejection into ClientError::Rejected then AdapterOperationFailure; domain_result L64 passes OPERATION_FAILED data through as the tool error L78; typed_failure_result L95 round-trips rejection data through from_value::<TTypedFailure> then to_value L111-L112; success_result L121 wraps McpToolOutput::Success; operation_failure_result L127; call_abandoned_at_shutdown L135 and caller_cancelled L146 hand-build error bodies (json! L136, L147).",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/board_argument_classification.rs",
      disposition: { kind: "converts", owners: ["mcp", "router-vocabulary"] },
      note: "decode_board_arguments L25 decodes board tool arguments or classifies the failure into BoardError: classify_failure L80 re-parses the raw argument object field by field by wire name (about L86, actor/reader identities L119-L209, UUIDv7 fields L211, feed cursors L299, metadata L356, page L407, repository L432). UndecodedBoardArguments L35 carries a manual JsonSchema impl L57/L70 publishing the typed request schema; ACTOR_METHODS L120 and READER_METHODS L149 are tool-name tables; validate_reader L189 and classify_failure L84 match tool-name literals.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/board_variant_validation.rs",
      disposition: { kind: "reads", owners: ["mcp"] },
      note: "Declared by #[path] at board_argument_classification.rs:18-19. validate_closed_variants L10 decodes single argument fields into closed message_board unions (PostTarget, DiscoveryScope, PostListScope, InboxScope, SubscriptionTarget, PostReferences ...) keyed by tool-name literals L19-L104, to name the failing field.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/carrier_tools.rs",
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "Tools conversation_create L20, conversation_load L64, conversation_prompt L105, conversation_cancel L153, conversation_create_and_prompt L185, events_observe L205; notify_observed_event L276 sends the observationEvent custom notification (to_value L280, CustomNotification L285) while events_observe runs. The ACP and Codex frames these tools cause are written by collaboration-client, not here.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/catalog_descriptions.rs",
      disposition: { kind: "defines", owners: ["mcp"] },
      note: "operation_description L1: tool description text table keyed by tool-name literal, with a generic fallback for unknown names.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/catalog_tools.rs",
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "Macros application_tool! L18 / domain_tool! L51 / board_tool! L69 register and answer tools; registrations: automation mutation L115-L136, automation inspection L149-L160, board L187-L281 (project_claim L255 takes board_sync_protocol::ProjectClaimRequest and answers ProjectClaimReply). Schemas via schema_for_type (L24, L26, L77) and board_input_schema L75; EmptyToolInput L284 (Deserialize, deny_unknown_fields).",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/conversation_operation_tools.rs",
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "conversation_operation_tool! L8 registers and answers conversation_operation_show L70, _wait L71, _reconcile L72 (schema_for_type L14, L16; typed_failure_result into ConversationOperationFailure L39); operation_call_cancelled L53 hand-builds the cancelled reply (json! L54).",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/conversation_tool_requests.rs",
      disposition: { kind: "defines", owners: ["mcp"] },
      note: "Typed inbound tool arguments ConversationCreateToolRequest L21, ConversationLoadToolRequest L30, ConversationPromptToolRequest L39, ConversationCreatePromptToolRequest L58 (flatten the collaboration-client inputs plus timeoutSeconds); hand-written schemars extend(allOf ...) L11 and L48 hard-code per-endpoint required fields for codex-local, claude-local, cursor-local.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/conversation_tool_results.rs",
      disposition: { kind: "converts", owners: ["mcp"] },
      note: "conversation_call_cancelled L11 and conversation_tool_result L29 build structured error bodies by hand: json! L15, L62, L72, L96, L104; AfterCreate patches createOperationId and target into an existing body L49-L54; Rejected data is round-tripped through from_value::<ConversationOperationFailure> then to_value L83-L86.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/interaction_tools.rs",
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "Tools approval_list L13, approval_decide L24, question_list L40, question_answer L51, conversation_settings_set L67, conversation_settings_accept L78, conversation_resume L92, conversation_close L106, automation_status L120; staged_failure_result L131 overwrites the failure stage with the literals approval-decision L36 / question-answer L63.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/message_tools.rs",
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "Tools message_send L14, router_show L59, message_inbox L70, message_history L81, message_reply L92. message_send re-decodes itemId alone L25 to pick invalidShape, then maps MessageSendRequest field by field into SessionMessageSendParams L46 (delivery to mode); message_reply maps MessageReplyRequest into SessionMessageReplyParams L98; rejected_before_submission L134 maps MessageFailureKind to an effect. UndecodedMessageSend L155 has a manual JsonSchema impl L156/L166 that publishes MessageSendRequest.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/project_tool_schemas.rs",
      disposition: { kind: "converts", owners: ["mcp"] },
      note: "board_input_schema L8 and narrow_project_schema L22 rewrite generated schemars output after generation: variant selection by kind const L53, const injection L44, reachable-definition pruning L67, and a not-{} fallback L18.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/result_converters.rs",
      disposition: { kind: "converts", owners: ["mcp"] },
      note: "structured_tool_error L21 projects bodies through McpToolError::from_existing; operation_error_result L32 and failure_naming L49 patch target/turnId/session fields into a serialized failure (json! L40-L41, L57); local receipt converters L73, L107 map DeliveryOutcome to kind/message/effect and receipt_error L133 patches them in (L142-L144); thread_wait_outcome_unknown L152 (json! L157), validation_failure L178 (json! L179); structured_argument_refusal L193 turns rmcp's text-only argument refusal into a protocolViolation body.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/schema_binding.rs",
      disposition: { kind: "converts", owners: ["mcp", "codex"] },
      note: "bind_native_schema_refs L3 rewrites urn:codex-native:<Name> refs to #/$defs/codexNative<Name> using the Host's exported Codex v2 definitions (rebase L83, replace L106; an empty schema when no definitions); same technique as collaboration-protocol cli_output_contract.rs:133.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/session_tools.rs",
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "Tools installation_show L15 (InstallationIdentity, whose central observation nests board_sync_protocol::FleetLinkRecord: collaboration-protocol installation_peer_identity.rs:31), endpoints_list L26, sessions_list L37 (Claude rejection data json! L46), provider_sessions_list L64, provider_session_inspect L78, session_inspect L92, session_rename L106, turn_interrupt L120, journal_status L134, journal_read L142, addresses_list L157.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/snapshots/main_success_schemas.json",
      disposition: { kind: "none" },
      note: "Test baseline snapshot of advertised MCP tool success schemas; read only by the cfg(test) mcp_server/tests/inspect_snapshot_tests.rs:154 (include_str).",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at mcp_server.rs:378 under #[cfg(test)] (mcp_server.rs:377)",
      },
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/tool_output_contract.rs",
      disposition: { kind: "defines", owners: ["mcp"] },
      note: "McpToolOutput L10 (untagged Success/Error) and McpToolError L17 (tag mcpResult; kind, message, stage, effect, target, operationId, flattened domain fields) give every tool output schema a structured error arm; from_existing L36 projects hand-built json! error bodies into it, defaulting kind to protocolViolation L53.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server/wait_tools.rs",
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "Tools thread_wait L19 (UndecodedBoardArguments<ThreadSubscriptionWaitRequest> L28) and wake_wait_until_first_fire L54; the wake wait maps WakeWaitFailure into collaboration_client::WakeWaitError L72-L77.",
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server_node_tap_session_inspect_tool.json",
      disposition: { kind: "none" },
      note: "Test baseline: the session_inspect tool as advertised without Codex definitions; read by include_str in the cfg(test) N11 node tap test (mcp_server_node_tap_test.rs:523, module declared at mcp_server.rs:380-382).",
    },
    {
      file: "crates/collaboration-mcp/src/native_schema_definitions.rs",
      disposition: { kind: "reads", owners: ["codex"] },
      note: "NativeSchemaDefinitions::from_bundle L17 extracts /documents/codex_app_server_protocol.schemas.json/definitions/v2 (L20) from the Host's exported Codex bundle.",
    },
    {
      file: "crates/collaboration-mcp/src/provider_conversation_http_tests/scripted_provider_backend.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared by #[path] at provider_conversation_http_tests.rs:10-11, itself a #[cfg(test)] module at lib.rs:43-44",
      },
    },
    {
      file: "crates/collaboration-mcp/src/proxy_authority.rs",
      disposition: { kind: "none" },
      note: "Proxy endpoint authority check and Host header normalization (normalize_default_https_host L52); no message shapes.",
    },
    {
      file: "crates/collaboration-mcp/src/router_executable_check.rs",
      disposition: { kind: "none" },
      note: "RouterExecutableCheck trait L7 and a fixed relation L13; no message shapes.",
    },
    {
      file: "crates/collaboration-mcp/src/test_support.rs",
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "Test apparatus behind the test-support feature (lib.rs:20-21), used as a dev-dependency by agent-collaboration, agent-session-inventory, codex-router-host, collaboration-client, collaboration-service and collaboration-application-test-support. call_projecting_failure L146 builds an rmcp tools/call ClientJsonRpcMessage (L155-L161) and physically posts it over a real UnixSocketHttpClient to the served MCP endpoint (L167-L175; listed as an apparatus send); it decodes SSE data lines with the canonical rmcp ServerJsonRpcMessage (L180-L195) and projects the decoded answer into a {result} / {error:{code,message,data}} envelope for test callers (L203-L224); published_error L236 re-reads a structured tool error's kind/code/message/data by hand into a JSON-RPC rejection (default code -32050 L246). The returned projection is not a capture of the connection.",
    },
    {
      file: "crates/collaboration-mcp/src/tool_call_registry.rs",
      disposition: { kind: "none" },
      note: "Tracks in-flight tool calls for shutdown (CallEndedEarly L17, ToolCallRegistry L25); no message shapes.",
    },
  ],
  sendSites: [
    {
      file: "crates/collaboration-mcp/src/collaboration_api_router.rs",
      line: 257,
      edge: "mcp",
      sends:
        "tools/call result kind overloaded for a shed call (overloaded_tool_result L279), written by json_message_response L263",
      owner: N11,
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason: "No tap case sheds a request; the N11 tap does not exercise admission.",
      },
    },
    {
      file: "crates/collaboration-mcp/src/collaboration_api_router.rs",
      line: 261,
      edge: "mcp",
      sends:
        "JSON-RPC error -32050 with data kind overloaded for any other shed request (overloaded_error L288), written by json_message_response L263",
      owner: N11,
      origin: "product",
      coverage: { kind: "uncovered", reason: "No tap case sheds a request." },
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server.rs",
      line: 305,
      edge: "mcp",
      sends:
        "initialize result, with router_build_warning instructions when the Router is stale (L313-L315); the tap harness fixes the executable relation to Match (api_test_harness.rs:51-62), so only the no-warning branch is recorded",
      owner: N11,
      origin: "product",
      coverage: { kind: "tap", testFile: N11_TAP, cases: HANDSHAKE_CASES },
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server.rs",
      line: 342,
      edge: "mcp",
      sends:
        "tools/call reply callerCancelled when the caller disconnects mid-call (caller_cancelled)",
      owner: N11,
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason: "No N11 tap case disconnects a caller; a disconnected caller reads no reply.",
      },
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server.rs",
      line: 345,
      edge: "mcp",
      sends:
        "tools/call reply unavailable/shutdown for a call aborted at shutdown (call_abandoned_at_shutdown)",
      owner: N11,
      origin: "product",
      coverage: { kind: "uncovered", reason: "No N11 tap case stops the listener mid-call." },
    },
    {
      file: "crates/collaboration-mcp/src/mcp_server.rs",
      line: 357,
      edge: "mcp",
      sends:
        "tools/list result (schemas resolved by resolve_tool_schemas L195); the wire cases run without Codex definitions, so only the opaque variant crosses the wire",
      owner: N11,
      origin: "product",
      coverage: { kind: "tap", testFile: N11_TAP, cases: HANDSHAKE_CASES },
    },
    toolReply("carrier_tools.rs", 20, "conversation_create"),
    toolReply("carrier_tools.rs", 64, "conversation_load"),
    toolReply("carrier_tools.rs", 105, "conversation_prompt", {
      kind: "tap",
      testFile: N11_TAP,
      cases: ["acp_carrier_tool_to_n8"],
    }),
    toolReply("carrier_tools.rs", 153, "conversation_cancel", {
      kind: "tap",
      testFile: N11_TAP,
      cases: ["session_command_tool_to_n4"],
    }),
    toolReply("carrier_tools.rs", 185, "conversation_create_and_prompt"),
    toolReply("carrier_tools.rs", 205, "events_observe"),
    {
      file: "crates/collaboration-mcp/src/mcp_server/carrier_tools.rs",
      line: 284,
      edge: "mcp",
      sends: "notifications/codexRouter/observationEvent custom notification during events_observe",
      owner: N11,
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "The tap cases do not run an observation; carrier_tools.rs notification_delivery_tests and observation_streaming_tests are unit tests, not node taps.",
      },
    },
    macroToolReply("catalog_tools.rs", 115, "instruction_create", "domain_tool"),
    macroToolReply("catalog_tools.rs", 116, "instruction_update", "domain_tool"),
    macroToolReply("catalog_tools.rs", 117, "instruction_show", "domain_tool"),
    macroToolReply("catalog_tools.rs", 118, "wake_send", "domain_tool"),
    macroToolReply("catalog_tools.rs", 119, "wake_show", "domain_tool"),
    macroToolReply("catalog_tools.rs", 120, "wake_pause", "domain_tool"),
    macroToolReply("catalog_tools.rs", 121, "wake_resume", "domain_tool"),
    macroToolReply("catalog_tools.rs", 122, "wake_cancel", "domain_tool"),
    macroToolReply("catalog_tools.rs", 123, "delivery_show", "domain_tool"),
    macroToolReply("catalog_tools.rs", 124, "wake_list", "domain_tool"),
    macroToolReply("catalog_tools.rs", 125, "schedule_import", "domain_tool"),
    macroToolReply("catalog_tools.rs", 126, "schedule_export", "domain_tool"),
    macroToolReply("catalog_tools.rs", 127, "schedule_create", "domain_tool"),
    macroToolReply("catalog_tools.rs", 128, "schedule_update", "domain_tool"),
    macroToolReply("catalog_tools.rs", 129, "schedule_show", "domain_tool"),
    macroToolReply("catalog_tools.rs", 130, "schedule_enable", "domain_tool"),
    macroToolReply("catalog_tools.rs", 131, "schedule_disable", "domain_tool"),
    macroToolReply("catalog_tools.rs", 132, "schedule_prepare", "domain_tool"),
    macroToolReply("catalog_tools.rs", 133, "run_show", "domain_tool"),
    macroToolReply("catalog_tools.rs", 134, "run_summary_retry", "domain_tool"),
    macroToolReply("catalog_tools.rs", 135, "run_summary_skip", "domain_tool"),
    macroToolReply("catalog_tools.rs", 136, "automation_configure", "domain_tool"),
    macroToolReply("catalog_tools.rs", 149, "run_reconcile", "domain_tool"),
    macroToolReply("catalog_tools.rs", 150, "delivery_reconcile", "domain_tool"),
    macroToolReply("catalog_tools.rs", 151, "operation_show", "domain_tool"),
    macroToolReply("catalog_tools.rs", 152, "operation_reconcile", "domain_tool"),
    macroToolReply("catalog_tools.rs", 153, "automation_events", "domain_tool"),
    macroToolReply("catalog_tools.rs", 154, "delivery_attempts", "domain_tool"),
    macroToolReply("catalog_tools.rs", 155, "run_summaries", "domain_tool"),
    macroToolReply("catalog_tools.rs", 156, "instruction_list", "domain_tool"),
    macroToolReply("catalog_tools.rs", 157, "schedule_list", "domain_tool"),
    macroToolReply("catalog_tools.rs", 158, "run_list", "domain_tool"),
    macroToolReply("catalog_tools.rs", 159, "delivery_list", "domain_tool"),
    macroToolReply("catalog_tools.rs", 160, "revision_list", "domain_tool"),
    macroToolReply("catalog_tools.rs", 187, "request_effects", "board_tool"),
    macroToolReply("catalog_tools.rs", 188, "workstream_create", "board_tool"),
    macroToolReply("catalog_tools.rs", 189, "workstream_rename", "board_tool"),
    macroToolReply("catalog_tools.rs", 190, "workstream_close", "board_tool"),
    macroToolReply("catalog_tools.rs", 191, "task_create", "board_tool"),
    macroToolReply("catalog_tools.rs", 192, "task_update", "board_tool"),
    macroToolReply("catalog_tools.rs", 193, "task_move_workstream", "board_tool"),
    macroToolReply("catalog_tools.rs", 194, "task_set_status", "board_tool"),
    macroToolReply("catalog_tools.rs", 195, "task_link", "board_tool"),
    macroToolReply("catalog_tools.rs", 196, "task_unlink", "board_tool"),
    macroToolReply("catalog_tools.rs", 197, "task_duplicate", "board_tool"),
    macroToolReply("catalog_tools.rs", 198, "task_replace", "board_tool"),
    macroToolReply("catalog_tools.rs", 199, "subtask_add", "board_tool"),
    macroToolReply("catalog_tools.rs", 200, "subtask_check", "board_tool"),
    macroToolReply("catalog_tools.rs", 201, "subtask_uncheck", "board_tool"),
    macroToolReply("catalog_tools.rs", 202, "subtask_move", "board_tool"),
    macroToolReply("catalog_tools.rs", 203, "subtask_remove", "board_tool"),
    macroToolReply("catalog_tools.rs", 204, "work_trail_add", "board_tool"),
    macroToolReply("catalog_tools.rs", 205, "work_trail_redact", "board_tool"),
    macroToolReply("catalog_tools.rs", 206, "role_grant", "board_tool"),
    macroToolReply("catalog_tools.rs", 207, "role_release", "board_tool"),
    macroToolReply("catalog_tools.rs", 208, "owner_grant_record", "board_tool"),
    macroToolReply("catalog_tools.rs", 209, "role_replace_lead", "board_tool"),
    macroToolReply("catalog_tools.rs", 210, "role_acknowledge_current", "board_tool"),
    macroToolReply("catalog_tools.rs", 211, "project_publish_plan", "board_tool"),
    macroToolReply("catalog_tools.rs", 212, "project_close", "board_tool"),
    macroToolReply("catalog_tools.rs", 213, "project_describe", "board_tool"),
    macroToolReply("catalog_tools.rs", 214, "fact_record", "board_tool"),
    macroToolReply("catalog_tools.rs", 215, "thread_renew", "board_tool"),
    macroToolReply("catalog_tools.rs", 216, "workstream_show", "board_tool"),
    macroToolReply("catalog_tools.rs", 217, "workstream_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 218, "task_show", "board_tool"),
    macroToolReply("catalog_tools.rs", 219, "task_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 220, "subtask_show", "board_tool"),
    macroToolReply("catalog_tools.rs", 221, "subtask_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 222, "role_show", "board_tool"),
    macroToolReply("catalog_tools.rs", 223, "role_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 224, "fact_show", "board_tool"),
    macroToolReply("catalog_tools.rs", 225, "fact_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 226, "intent_show", "board_tool"),
    macroToolReply("catalog_tools.rs", 227, "intent_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 228, "adr_create", "board_tool"),
    macroToolReply("catalog_tools.rs", 229, "adr_transition", "board_tool"),
    macroToolReply("catalog_tools.rs", 230, "adr_show", "board_tool"),
    macroToolReply("catalog_tools.rs", 231, "adr_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 232, "feed_read", "board_tool"),
    macroToolReply("catalog_tools.rs", 233, "view_work", "board_tool"),
    macroToolReply("catalog_tools.rs", 234, "view_card", "board_tool"),
    macroToolReply("catalog_tools.rs", 235, "view_session_context", "board_tool"),
    macroToolReply("catalog_tools.rs", 236, "view_owner", "board_tool"),
    macroToolReply("catalog_tools.rs", 237, "view_lead_load", "board_tool"),
    macroToolReply("catalog_tools.rs", 238, "work_trail_show", "board_tool"),
    macroToolReply("catalog_tools.rs", 239, "discovery_search", "board_tool"),
    macroToolReply("catalog_tools.rs", 240, "post_search", "board_tool"),
    macroToolReply("catalog_tools.rs", 241, "workspace_create", "board_tool"),
    macroToolReply("catalog_tools.rs", 242, "workspace_archive", "board_tool"),
    macroToolReply("catalog_tools.rs", 243, "workspace_update", "board_tool"),
    macroToolReply("catalog_tools.rs", 244, "workspace_show", "board_tool"),
    macroToolReply("catalog_tools.rs", 245, "workspace_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 246, "repository_attach", "board_tool"),
    macroToolReply("catalog_tools.rs", 247, "repository_detach", "board_tool"),
    macroToolReply("catalog_tools.rs", 248, "repository_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 249, "board_create", "board_tool"),
    macroToolReply("catalog_tools.rs", 250, "board_update", "board_tool"),
    macroToolReply("catalog_tools.rs", 251, "board_show", "board_tool"),
    macroToolReply("catalog_tools.rs", 252, "board_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 253, "board_close", "board_tool"),
    macroToolReply("catalog_tools.rs", 254, "board_archive", "board_tool"),
    macroToolReply("catalog_tools.rs", 255, "project_claim", "board_tool"),
    macroToolReply("catalog_tools.rs", 256, "project_create", "board_tool"),
    macroToolReply("catalog_tools.rs", 257, "project_update", "board_tool"),
    macroToolReply("catalog_tools.rs", 258, "project_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 259, "project_show", "board_tool"),
    macroToolReply("catalog_tools.rs", 260, "thread_post", "board_tool"),
    macroToolReply("catalog_tools.rs", 261, "post_show", "board_tool"),
    macroToolReply("catalog_tools.rs", 262, "post_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 263, "thread_show", "board_tool"),
    macroToolReply("catalog_tools.rs", 264, "thread_resolve", "board_tool"),
    macroToolReply("catalog_tools.rs", 265, "thread_unresolve", "board_tool"),
    macroToolReply("catalog_tools.rs", 266, "thread_watch", "board_tool"),
    macroToolReply("catalog_tools.rs", 267, "thread_unwatch", "board_tool"),
    macroToolReply("catalog_tools.rs", 268, "project_watch", "board_tool"),
    macroToolReply("catalog_tools.rs", 269, "project_unwatch", "board_tool"),
    macroToolReply("catalog_tools.rs", 270, "thread_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 271, "thread_create", "board_tool"),
    macroToolReply("catalog_tools.rs", 272, "thread_refile", "board_tool"),
    macroToolReply("catalog_tools.rs", 273, "thread_join", "board_tool"),
    macroToolReply("catalog_tools.rs", 274, "thread_leave", "board_tool"),
    macroToolReply("catalog_tools.rs", 275, "thread_participant_list", "board_tool"),
    macroToolReply("catalog_tools.rs", 276, "thread_subscribe", "board_tool"),
    macroToolReply("catalog_tools.rs", 277, "thread_unsubscribe", "board_tool"),
    macroToolReply("catalog_tools.rs", 278, "thread_subscriptions", "board_tool"),
    macroToolReply("catalog_tools.rs", 279, "inbox_fetch", "board_tool"),
    macroToolReply("catalog_tools.rs", 280, "inbox_acknowledge", "board_tool"),
    macroToolReply("catalog_tools.rs", 281, "inbox_workspaces", "board_tool"),
    macroToolReply(
      "conversation_operation_tools.rs",
      70,
      "conversation_operation_show",
      "conversation_operation_tool",
    ),
    macroToolReply(
      "conversation_operation_tools.rs",
      71,
      "conversation_operation_wait",
      "conversation_operation_tool",
    ),
    macroToolReply(
      "conversation_operation_tools.rs",
      72,
      "conversation_operation_reconcile",
      "conversation_operation_tool",
    ),
    toolReply("interaction_tools.rs", 13, "approval_list"),
    toolReply("interaction_tools.rs", 24, "approval_decide"),
    toolReply("interaction_tools.rs", 40, "question_list"),
    toolReply("interaction_tools.rs", 51, "question_answer"),
    toolReply("interaction_tools.rs", 67, "conversation_settings_set"),
    toolReply("interaction_tools.rs", 78, "conversation_settings_accept"),
    toolReply("interaction_tools.rs", 92, "conversation_resume"),
    toolReply("interaction_tools.rs", 106, "conversation_close"),
    toolReply("interaction_tools.rs", 120, "automation_status"),
    toolReply("message_tools.rs", 14, "message_send"),
    toolReply("message_tools.rs", 59, "router_show"),
    toolReply("message_tools.rs", 70, "message_inbox"),
    toolReply("message_tools.rs", 81, "message_history"),
    toolReply("message_tools.rs", 92, "message_reply"),
    toolReply("session_tools.rs", 15, "installation_show"),
    toolReply("session_tools.rs", 26, "endpoints_list"),
    toolReply("session_tools.rs", 37, "sessions_list"),
    toolReply("session_tools.rs", 64, "provider_sessions_list"),
    toolReply("session_tools.rs", 78, "provider_session_inspect"),
    toolReply("session_tools.rs", 92, "session_inspect"),
    toolReply("session_tools.rs", 106, "session_rename"),
    toolReply("session_tools.rs", 120, "turn_interrupt", {
      kind: "tap",
      testFile: N11_TAP,
      cases: ["native_control_tool_to_n10"],
    }),
    toolReply("session_tools.rs", 134, "journal_status"),
    toolReply("session_tools.rs", 142, "journal_read"),
    toolReply("session_tools.rs", 157, "addresses_list"),
    toolReply("wait_tools.rs", 19, "thread_wait"),
    toolReply("wait_tools.rs", 54, "wake_wait_until_first_fire"),
    // Caller sites (coordinator ruling): each call in this crate into a library sender that
    // writes on an outside edge is its own row. Writers: collaboration-client AcpConversation
    // (acp_conversation.rs, acp_session_setup.rs) toward the Codex ACP adapter (N8);
    // codex-native-integration NativeProtocolConnection toward Codex (N10, through the
    // application's native backend); the provider supervisor toward provider ACP agents
    // (through the application's conversation operations).
    callerSite(
      "mcp_server/carrier_tools.rs",
      40,
      "acp-served-n8",
      "ACP initialize from ConversationClient::connect (conversation_create, Codex ACP endpoint; AcpConversation::connect_with_context)",
    ),
    callerSite(
      "mcp_server/carrier_tools.rs",
      48,
      "acp-served-n8",
      "ACP session/new from ConversationClient::create (conversation_create, Codex ACP endpoint)",
    ),
    callerSite(
      "mcp_server/carrier_tools.rs",
      78,
      "acp-served-n8",
      "ACP initialize from ConversationClient::connect (conversation_load, Codex ACP endpoint)",
    ),
    callerSite(
      "mcp_server/carrier_tools.rs",
      94,
      "acp-served-n8",
      "ACP session/load from ConversationClient::load (conversation_load, Codex ACP endpoint)",
    ),
    callerSite(
      "mcp_server/carrier_tools.rs",
      119,
      "acp-served-n8",
      "ACP initialize from ConversationClient::connect (conversation_prompt, Codex ACP endpoint)",
      ACP_ADAPTER_TAP,
    ),
    callerSite(
      "mcp_server/carrier_tools.rs",
      148,
      "acp-served-n8",
      "ACP session/load before the prompt, from ConversationClient::prompt (conversation_prompt, Codex ACP endpoint)",
      ACP_ADAPTER_TAP,
    ),
    callerSite(
      "mcp_server/carrier_tools.rs",
      148,
      "acp-served-n8",
      "ACP session/prompt from ConversationClient::prompt (conversation_prompt, Codex ACP endpoint)",
      {
        kind: "uncovered",
        reason:
          "acp_carrier_tool_to_n8 has the fake adapter refuse session/load (mcp_server_node_tap_wire.rs:172), so no session/prompt is written in any node tap.",
      },
    ),
    callerSite(
      "mcp_server/carrier_tools.rs",
      165,
      "acp-served-n8",
      "ACP initialize from ConversationClient::connect (conversation_cancel, Codex ACP endpoint; cancel itself then refuses with unsupportedCapability without a send, conversation_session_operations.rs:346)",
    ),
    callerSite(
      "mcp_server/carrier_tools.rs",
      195,
      "acp-served-n8",
      "ACP initialize, session/new and session/prompt from ConversationClient::create_and_prompt (conversation_create_and_prompt, Codex ACP endpoint)",
    ),
    callerSite(
      "mcp_server/carrier_tools.rs",
      213,
      "cli-codex",
      "Codex native attach and observation requests from SessionObservation::observe_bounded (events_observe on the Router-published NativeCodex channel, edge cli-codex as for collaboration-client observation_session.rs:213; observation_session.rs:151)",
    ),
    callerSite(
      "local_collaboration.rs",
      54,
      "acp-agent",
      "provider conversation create, admitted to the provider supervisor (conversation_create / create_and_prompt on a provider endpoint)",
    ),
    callerSite(
      "local_collaboration.rs",
      67,
      "acp-agent",
      "provider conversation load, admitted to the provider supervisor (conversation_load on a provider endpoint)",
    ),
    callerSite(
      "local_collaboration.rs",
      80,
      "acp-agent",
      "provider conversation prompt, admitted to the provider supervisor (conversation_prompt on a provider endpoint)",
    ),
    callerSite(
      "local_collaboration.rs",
      93,
      "acp-agent",
      "provider conversation cancel, admitted to the provider supervisor (conversation_cancel on a provider endpoint)",
      {
        kind: "uncovered",
        reason:
          "session_command_tool_to_n4 records the provider-backend command at RecordLayer::TypedOutcome (mcp_server_node_tap_test.rs:288) against a recording backend; no ACP agent frame is recorded at WireFrame.",
      },
    ),
    callerSite(
      "mcp_server/interaction_tools.rs",
      73,
      "acp-agent",
      "provider settings request (mode, model or effort) from conversation_settings_set",
    ),
    callerSite(
      "mcp_server/interaction_tools.rs",
      100,
      "acp-agent",
      "provider session resume from conversation_resume",
    ),
    callerSite(
      "mcp_server/interaction_tools.rs",
      114,
      "acp-agent",
      "provider session close from conversation_close",
    ),
    callerSite(
      "mcp_server/session_tools.rs",
      58,
      "managed-codex",
      "Codex thread list requests from codex_session_list (sessions_list)",
    ),
    callerSite(
      "mcp_server/session_tools.rs",
      100,
      "managed-codex",
      "Codex initialize/initialized and thread/read from codex_session_inspect (session_inspect): collaboration-service session_native_control.rs:45-57 native_call with NativeOperation::ReadThread, sent through NativeProtocolConnection::connect and request_validated (:302, :312), not the native_thread_operations.rs:33 helper",
    ),
    callerSite(
      "mcp_server/session_tools.rs",
      114,
      "managed-codex",
      "Codex thread/name/set from codex_session_rename (session_rename)",
    ),
    callerSite(
      "mcp_server/session_tools.rs",
      128,
      "managed-codex",
      "Codex turn/interrupt from codex_turn_interrupt (turn_interrupt): collaboration-service session_native_control.rs:106-122 native_call with NativeOperation::InterruptTurn, sent through NativeProtocolConnection::connect and request_validated (:302, :312) after the N10 initialize/initialized handshake; not the native_thread_operations.rs:56 helper, which sends the same method from another call site",
      {
        kind: "tap",
        testFile: N11_TAP,
        cases: ["native_control_tool_to_n10"],
      },
    ),
    {
      file: "crates/collaboration-mcp/src/test_support.rs",
      line: 167,
      edge: "mcp",
      sends:
        "tools/call request (rmcp CallToolRequest built L155-L161) posted over UnixSocketHttpClient to the served MCP endpoint by the test-support McpClient (call_projecting_failure L146), on behalf of the test callers in agent-collaboration, agent-session-inventory, codex-router-host, collaboration-client, collaboration-service and collaboration-application-test-support (board_fixture)",
      owner: { crate: "collaboration-mcp" },
      origin: "apparatus",
      coverage: {
        kind: "uncovered",
        reason:
          "No kept capture records this connection: the N11 node tap posts its own tools/call frames through api_test_harness (listener.post), not this client, and call_projecting_failure returns only a projection of the decoded answer.",
      },
    },
  ],
};
