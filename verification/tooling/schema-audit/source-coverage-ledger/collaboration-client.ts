import type { LedgerSection, SendSite } from "../schema-audit-types.ts";
// Left out by name only: 12 colocated unit-test files (*_test.rs, *_tests.rs).

const R = "crates/collaboration-client/src/";
const CLIENT = { crate: "collaboration-client" } as const;
const ACP_TAP = "crates/collaboration-mcp/src/mcp_server_node_tap_test.rs";
const noTap = (reason: string) => ({ kind: "uncovered", reason }) as const;
const CALLER_SIDE = noTap(
  "Caller side of an MCP tools/call: no node tap drives this client; the N11 taps post their own tools/call frames to the served API (mcp_server_node_tap_test.rs listener.post), so nothing records what this client writes.",
);
/** One calling site that names one collaboration API tool; the frame itself is written at api_connection.rs:141. */
const toolCall = (file: string, line: number, tool: string): SendSite => ({
  file: `${R}${file}`,
  line,
  edge: "mcp",
  sends: `tools/call ${tool}`,
  owner: CLIENT,
  origin: "product",
  coverage: CALLER_SIDE,
});

export const section: LedgerSection = {
  root: "crates/collaboration-client",
  files: [
    {
      file: `${R}acp_conversation.rs`,
      disposition: { kind: "sends", owners: ["acp", "router-extension"] },
      note: 'ACP client of the Router-served Codex adapter (N8) over a Unix stream: initialize built L289 and sent L294, session/prompt L339 (_meta.codexRouter effort L341, L691), permission cancel reply L501; submit L447 writes the JSON-RPC envelope; handle_callback L476 reads session/update and session/request_permission; reads protocolVersion L303, agentCapabilities.loadSession L310 and _meta[codex-router/finalReply] L583 by hand; validates every frame against the vendored ACP schema L519; after connect, compares the transport\'s advertised schema digest with format!("sha256:{}", ACP_SCHEMA_DIGEST) L256-L257, a hand encoder of the ACP digest text (inventory row :256).',
    },
    {
      file: `${R}acp_session_setup.rs`,
      disposition: { kind: "sends", owners: ["acp", "router-extension"] },
      note: "Child module of acp_conversation.rs (declared acp_conversation.rs:703, not cfg(test)). session/load params L91 (_meta.codexRouter L84, _meta.router.sessionRef L87, _meta[codex-router/replayHistory] L89) sent L97; session/new params L167 (_meta.codexRouter model, effort, access, createdBy, approver, rootMessageId, scratchScope, scratchPath, operationId, forkThreadId, mode L120-L164) sent L176; reads sessionId L196, _meta.codexRouter.permissionMode L237 and permissionDifferences L249.",
    },
    {
      file: `${R}acp_transport_connection.rs`,
      disposition: { kind: "none" },
      note: "Selects the advertised ACP channel from the endpoint inventory and connects its Unix socket (L64); sends no ACP message.",
    },
    {
      file: `${R}admission_overload.rs`,
      disposition: { kind: "converts", owners: ["mcp", "router-vocabulary"] },
      note: "admission_overload L28 recognises the generic overloaded tool failure {kind, effect, message} by hand; ShedRequest L44 copies operationId/wakeupId/scheduleId/runId/instructionId out of request JSON (from_value L67); L77-L180 rebuild that one body as eight family failures (board, wake, schedule, run, instruction, configuration, inspection, conversation).",
    },
    {
      file: `${R}api_connection.rs`,
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "The one MCP caller transport: exchange L116 builds a tools/call request (rmcp CallToolRequestParams L130) and posts it L141 over the Unix-socket streamable HTTP client; SSE data lines decoded as ServerJsonRpcMessage L154; ToolAnswer L31 from structuredContent/isError L179-L186; rejection_from_tool_failure L203 reads the tool error body.",
    },
    {
      file: `${R}automation_configuration_operations.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "automation_configure L19 and automation_status L23 through configuration_call L26 (connection.call L34); code -32050 data decoded as ConfigurationFailure L36-L43.",
    },
    {
      file: `${R}automation_inspection_operations.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "Twelve inspection tools (L24-L131) through automation_inspection_call L134 (connection.call L144); code -32050 data decoded as AutomationInspectionFailure L146-L153.",
    },
    {
      file: `${R}central_admin.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "Owner HTTP calls to the central admin API (reqwest, /admin/v1/copies and /admin/v1/projects paths L94-L118, cf-access-token header L128); the shapes are collaboration_central_admin_protocol CopyPageQuery, OwnerMove, OwnerTakeover, OwnerRetirement and the untagged AdminReply L206, a central admin protocol between the owner CLI and central, not one of the six outside edges or four protocols.",
      },
      note: "Out of scope but carries serde shapes: AdminReply untagged decode L206-L220 and OwnerOperationFailure body decode L171.",
    },
    {
      file: `${R}collaboration_access.rs`,
      disposition: { kind: "none" },
      note: "LocalCollaboration trait L28 and CollaborationAccess L61: in-process calls carrying collaboration_protocol types, no serialization; endpoint_directory L96 opens the API client or the local Router.",
    },
    {
      file: `${R}collaboration_client.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "CollaborationClient: 15 tool calls by name literal (sessions_list L204 to installation_show L559); conversation_failure L50 decodes Rejected data; discovery stage strings mapped to PermissionDiagnosticStage L70-L78; inspect result thread.id read by hand L264; -32602 with no data read as an unknown tool L562; read_manifest L589 checks the version field by hand before the typed decode.",
    },
    {
      file: `${R}conversation_actor.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "ConversationActor L11 with a hand Deserialize L16 choosing SessionRef or message_board Identity by the presence of kind; provider_identity L45 converts both to ProviderIdentity.",
    },
    {
      file: `${R}conversation_client.rs`,
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "Public conversation inputs with schemars: ConversationCreateInput L28, ConversationLoadInput L59, ConversationPromptInput L74, ConversationCancelInput L92, ConversationCreatePromptInput L105; RouterAccess to access string L415; hand mapping to the protocol ConversationCreateRequest L476; conversation_create composite tool call L614; Codex mode literal auto L627.",
    },
    {
      file: `${R}conversation_contract.rs`,
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "ConversationEvent L12, ConversationCreateRequest L28 (access and mode as strings), ConversationPromptRequest L57, PublicPromptContent L80 with From into MessageContent L90, ConversationEnd L101.",
    },
    {
      file: `${R}conversation_operation_result.rs`,
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "ConversationOperationResult L19, ConversationCreatePromptOutcome L61, ConversationSettlement L81, ConversationStopReason L90 (adds Completed and TimedOut to the ACP stop reasons), ConversationSettlementDetail L108 (raw ACP result as Value L112), ProviderPromptOutput L133, ProviderLoadOutput L149.",
    },
    {
      file: `${R}conversation_session_operations.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "Hand mappings of public inputs to ConversationLoadRequest L106, ProviderPromptRequest L271 and ConversationCancelRequest L370; ConversationEnd to ConversationStopReason L228; provider_operation_result L390; provider_stop_reason L489; composite tool calls conversation_load L122, conversation_prompt L285, conversation_cancel L357.",
    },
    {
      file: `${R}instruction_operations.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "instruction_create L21, instruction_update L27, instruction_show L33 through instruction_call L35 (connection.call L43); code -32050 data decoded as InstructionFailure L45-L51.",
    },
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "Module list and re-exports; cfg(test) modules board_project_read_timeout_test L15-L16, scripted_api L104-L106, provider_conversation_operations_test L173-L174; router_version_warning L82 formats a stderr line.",
    },
    {
      file: `${R}message_operation.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "router_show L191, message_inbox L208, message_history L225 tool calls; MessageSendRequest L15 and MessageReplyRequest L27 with schemars; PublicMessageContent L95 with TryFrom/From MessageContent L105, L118; is_reply_pre_dispatch_rejection L232 reads failure kind strings.",
    },
    {
      file: `${R}message_submission.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "message_reply L46 and message_send L88 through delivery_receipt L223 (connection.answer L224, strips mcpResult/kind/message/effect L230); re-encodes SessionMessageSendParams as MessageSendRequest L85-L96; RouterLink from service id L128; delivery outcome and acceptance consistency table L141-L212.",
    },
    {
      file: `${R}native_endpoint_selector.rs`,
      disposition: { kind: "none" },
      note: "Resolves the public native socket path from the endpoint inventory before native TUI launch (codex-local literal L62); sends nothing.",
    },
    {
      file: `${R}native_transport_connection.rs`,
      disposition: { kind: "none" },
      note: "Opens the advertised native Codex WebSocket (client_async_with_config L133, NATIVE_WEBSOCKET_FRAME_LIMIT L11); sends no native protocol messages.",
    },
    {
      file: `${R}observation_session.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "NativeObservation L20 opens NativeProtocolConnection (codex-native-integration) to the published native socket L213, which sends initialize and initialized, then sends thread/resume through resume_thread L218; reads app-server frames as Value: next_message L262, collect_until L317 wraps each as ObservationEventNotification::native_event L356.",
    },
    {
      file: `${R}operation_error.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "operation_failure_from_client_error L84 maps ClientError to AdapterOperationFailure, reading kind/stage/message from failure data; hand-built diagnostics json! L108, L118 (ioKind, osCode) and overloaded body L136; failure kind/stage tables L194, L220.",
    },
    {
      file: `${R}owner_access.rs`,
      disposition: { kind: "none" },
      note: "Reads the owner Access token from cloudflared; no message shape.",
    },
    {
      file: `${R}owner_identity_resolution.rs`,
      disposition: { kind: "none" },
      note: "Resolves the owner HumanId by running /usr/bin/id -un with a timeout.",
    },
    {
      file: `${R}provider_conversation_operations.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "Eight provider conversation tool calls by literal: provider_session_inspect L24 to conversation_operation_reconcile L99, conversation_operation_wait L85 with its own transport allowance.",
    },
    {
      file: `${R}provider_conversations.rs`,
      disposition: { kind: "converts", owners: ["mcp", "router-vocabulary"] },
      note: "composite_call L46 posts the composite conversation tool named by its caller (answer_with_timeout L53); conversation_failure L82 reads kind callerCancelled and operationId; composite_arguments L95 inserts timeoutSeconds into the serialized input; LocalProviderOperations L113.",
    },
    {
      file: `${R}provider_session_observation.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "events_observe tool call L345 through call_observing; custom notification method match and ObservationEventNotification decode L326-L339; event sequence read by hand L248.",
    },
    {
      file: `${R}run_operations.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "run_show L13, run_summary_retry L19, run_summary_skip L25 through run_call L27 (connection.call L35); code -32050 data decoded as RunFailure L37-L43.",
    },
    {
      file: `${R}schedule_operations.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "Eight schedule tool calls L19-L61 through schedule_call L63 (connection.call L85); operationId read from params by hand L76-L79; code -32050 data decoded as ScheduleFailure L87-L93.",
    },
    {
      file: `${R}service_directory.rs`,
      disposition: { kind: "none" },
      note: "Service directory path policy (agent-communication under .codex-router or .codex-router-debug); no message shape.",
    },
    {
      file: `${R}session_catalog.rs`,
      disposition: { kind: "none" },
      note: "Module list and re-exports for the session catalog.",
    },
    {
      file: `${R}session_catalog/history.rs`,
      disposition: { kind: "reads", owners: ["codex"] },
      note: "Bounded read of Codex rollout JSONL under CODEX_HOME/sessions: each line parsed as Value L128, conversation_message_from_event L162 keeps type response_item / payload.type message / role user|assistant and collects text, content, output_text L189.",
    },
    {
      file: `${R}session_catalog/query.rs`,
      disposition: { kind: "none" },
      note: "Keyset paging over Codex's state SQLite through codex-native-integration StoredThreadCatalog (row columns L113-L140) and a hand TOML line read of model_provider L189; Codex local state and config, not one of the four protocols.",
    },
    {
      file: `${R}session_catalog/records.rs`,
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "StoredSessionRecord L9 (Serialize): one stored Codex session returned by the catalog reader and printed by the CLI.",
    },
    {
      file: `${R}session_catalog/repository.rs`,
      disposition: { kind: "none" },
      note: "Git worktree and origin discovery for repository identity; no message shape.",
    },
    {
      file: `${R}wakeup_operations.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "Seven wake and delivery tool calls L17-L53 through wakeup_call L55 (connection.call L63); code -32050 data decoded as WakeFailure L65-L71.",
    },
    {
      file: `${R}wakeup_waiting.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "wake_wait_until_first_fire call L252; WakeWaitFailureKind L16, WakeWaitFailure L35 and WakeWaitEffects L49 (serde) built by hand L76-L160 and decoded from tool failures L343; cursor JSON-in-string parse L370.",
    },
  ],
  sendSites: [
    {
      file: `${R}acp_conversation.rs`,
      line: 294,
      edge: "acp-served-n8",
      sends: "initialize (params built L289)",
      owner: CLIENT,
      origin: "product",
      coverage: {
        kind: "tap",
        testFile: ACP_TAP,
        cases: ["node_tap_acp_carrier_tool_routes_to_codex_acp_adapter"],
      },
    },
    {
      file: `${R}acp_conversation.rs`,
      line: 339,
      edge: "acp-served-n8",
      sends: "session/prompt with _meta.codexRouter effort",
      owner: CLIENT,
      origin: "product",
      coverage: noTap(
        "acp_carrier_tool_to_n8 stops at a refused session/load, so no prompt is sent in any node tap.",
      ),
    },
    {
      file: `${R}acp_conversation.rs`,
      line: 447,
      edge: "acp-served-n8",
      sends:
        "JSON-RPC request envelope {jsonrpc, id, method, params} for every request this client submits",
      owner: CLIENT,
      origin: "product",
      coverage: {
        kind: "tap",
        testFile: ACP_TAP,
        cases: ["node_tap_acp_carrier_tool_routes_to_codex_acp_adapter"],
      },
    },
    {
      file: `${R}acp_conversation.rs`,
      line: 501,
      edge: "acp-served-n8",
      sends: "session/request_permission reply (cancelled outcome)",
      owner: CLIENT,
      origin: "product",
      coverage: noTap(
        "No node tap drives an agent request_permission callback through this client.",
      ),
    },
    {
      file: `${R}acp_session_setup.rs`,
      line: 97,
      edge: "acp-served-n8",
      sends:
        "session/load with _meta.codexRouter, _meta.router.sessionRef, _meta[codex-router/replayHistory]",
      owner: CLIENT,
      origin: "product",
      coverage: {
        kind: "tap",
        testFile: ACP_TAP,
        cases: ["node_tap_acp_carrier_tool_routes_to_codex_acp_adapter"],
      },
    },
    {
      file: `${R}acp_session_setup.rs`,
      line: 176,
      edge: "acp-served-n8",
      sends: "session/new with _meta.codexRouter",
      owner: CLIENT,
      origin: "product",
      coverage: noTap(
        "The only ACP tap that reaches this client (acp_carrier_tool_to_n8) ends at a refused session/load; no case sends session/new.",
      ),
    },
    {
      file: `${R}api_connection.rs`,
      line: 141,
      edge: "mcp",
      sends: "tools/call request frame (every collaboration API call below)",
      owner: CLIENT,
      origin: "product",
      coverage: CALLER_SIDE,
    },
    toolCall("collaboration_client.rs", 204, "sessions_list"),
    toolCall("collaboration_client.rs", 230, "provider_sessions_list"),
    toolCall("collaboration_client.rs", 258, "session_inspect"),
    toolCall("collaboration_client.rs", 279, "session_rename"),
    toolCall("collaboration_client.rs", 304, "turn_interrupt"),
    toolCall("collaboration_client.rs", 323, "approval_list (includeOptions false)"),
    toolCall("collaboration_client.rs", 340, "approval_list (includeOptions true)"),
    toolCall("collaboration_client.rs", 365, "approval_decide"),
    toolCall("collaboration_client.rs", 399, "question_list"),
    toolCall("collaboration_client.rs", 422, "question_answer"),
    toolCall("collaboration_client.rs", 448, "journal_status"),
    toolCall("collaboration_client.rs", 471, "addresses_list"),
    toolCall("collaboration_client.rs", 507, "journal_read"),
    toolCall("collaboration_client.rs", 544, "endpoints_list"),
    toolCall("collaboration_client.rs", 559, "installation_show"),
    toolCall("conversation_client.rs", 614, "conversation_create (composite)"),
    toolCall("conversation_session_operations.rs", 122, "conversation_load (composite)"),
    toolCall("conversation_session_operations.rs", 285, "conversation_prompt (composite)"),
    toolCall("conversation_session_operations.rs", 357, "conversation_cancel (composite)"),
    toolCall("message_operation.rs", 191, "router_show"),
    toolCall("message_operation.rs", 208, "message_inbox"),
    toolCall("message_operation.rs", 225, "message_history"),
    toolCall("message_submission.rs", 46, "message_reply"),
    toolCall("message_submission.rs", 88, "message_send"),
    {
      file: `${R}observation_session.rs`,
      line: 213,
      edge: "cli-codex",
      sends:
        "initialize and initialized, sent inside NativeProtocolConnection::connect to the Router-published NativeCodex channel (observation_session.rs:183)",
      owner: CLIENT,
      origin: "product",
      coverage: noTap(
        "No node tap attaches a NativeObservation; native_control_tool_to_n10 records collaboration-mcp's own native backend connection, not this client's.",
      ),
    },
    {
      file: `${R}observation_session.rs`,
      line: 218,
      edge: "cli-codex",
      sends: "thread/resume {threadId} through NativeProtocolConnection::resume_thread",
      owner: CLIENT,
      origin: "product",
      coverage: noTap(
        "No node tap attaches a NativeObservation, so no case sends this thread/resume.",
      ),
    },
    toolCall("provider_conversation_operations.rs", 24, "provider_session_inspect"),
    toolCall("provider_conversation_operations.rs", 32, "conversation_settings_set"),
    toolCall("provider_conversation_operations.rs", 40, "conversation_settings_accept"),
    toolCall("provider_conversation_operations.rs", 48, "conversation_resume"),
    toolCall("provider_conversation_operations.rs", 56, "conversation_close"),
    toolCall("provider_conversation_operations.rs", 66, "conversation_operation_show"),
    toolCall("provider_conversation_operations.rs", 85, "conversation_operation_wait"),
    toolCall("provider_conversation_operations.rs", 99, "conversation_operation_reconcile"),
    toolCall("provider_session_observation.rs", 345, "events_observe (streaming)"),
  ],
};
