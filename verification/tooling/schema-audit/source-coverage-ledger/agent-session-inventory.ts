import type { LedgerSection, SendSite } from "../schema-audit-types.ts";
// Left out by name only: colocated unit-test files (*_test.rs, *_tests.rs).

const R = "crates/agent-session-inventory/src/";
const S = `${R}session_commands/`;
const T = `${R}test_support/`;
const CRATE = { crate: "agent-session-inventory" } as const;
const noTap = (reason: string) => ({ kind: "uncovered", reason }) as const;
const CALLER_SIDE = noTap(
  "Caller side of an MCP tools/call made through collaboration-client (frame written at collaboration-client api_connection.rs:141): no node tap drives agent-session-inventory (no *node_tap* file references it); the N11 taps post their own tools/call frames (mcp_server_node_tap_test.rs:283 listener.post).",
);
const STAND_IN = noTap(
  "test apparatus: a test-support stand-in or relay used only by this crate's tests (feature test-support, lib.rs:11); no node tap records its connection at the WireFrame layer, and RecordedApiListener keeps request bodies only (recorded_api_listener.rs:137-149), not the replies, so no complete kept capture exists.",
);
const CONFIGURED_HTTP = noTap(
  "Caller side of a tools/call this crate writes itself: HttpConfiguredInventoryClient::read_call builds the rmcp CallToolRequest and POSTs it with its own reqwest client (configured_inventory_transport.rs:119-145), not through collaboration-client; no node tap drives it (the N11 taps post their own tools/call frames, mcp_server_node_tap_test.rs:283 listener.post), and the configured-source tests reach it through RecordedApiListener, which keeps request bodies only (recorded_api_listener.rs:137-149), not a complete connection.",
);
/** One calling site in this crate that triggers one collaboration API tool call. */
const toolCall = (file: string, line: number, sends: string): SendSite => ({
  file: `${S}${file}`,
  line,
  edge: "mcp",
  sends,
  owner: CRATE,
  origin: "product",
  coverage: CALLER_SIDE,
});

export const section: LedgerSection = {
  root: "crates/agent-session-inventory",
  files: [
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "module list; test_support declared under cfg(any(test, feature = test-support)) L11; no message shapes",
    },
    {
      file: `${R}picker_data_query.rs`,
      disposition: { kind: "none" },
      note: "SessionsPickerDataQuery / SessionsPickerRoot: picker query state, no serde, not one of the four protocols",
    },
    {
      file: `${R}picker_runtime_status.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "NativeLiveReadState::from_observation L128 validates the provider_sessions_list nativeCoverage and re-parses conflict SessionIds as UuidIdentity L138; PickerRuntimeStatus::from_provider L185 and from_native L196 hand-map ProviderSessionState and NativeThreadStatus",
    },
    {
      file: `${S}claude_launch_target.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary", "mcp"] },
      note: "active_claude_session_record L457 hand-builds picker JSON from ProviderSessionSummary (json! L470, L494), nativeConflict via to_value L81, read back by key L590/L610; literal claude-local EndpointRef L368; provider_sessions_list call L382; service.json read as ServiceManifest L533; /healthz HTTP preflight L544 is the Router proxy, not an edge",
    },
    {
      file: `${S}claude_resume_inventory.rs`,
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "complete_resume_inventory merges Loaded+Stored ProviderSessionSummary pages; resume_candidates reads hand records by key sessionId/project L46-L50 and flattens SessionIds to String L66/L72",
    },
    {
      file: `${S}claude_session_commands.rs`,
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "write_claude_session_table reads the hand-built picker JSON keys sessionId, source, name, project L90-L101; JSON output L75 writes those records",
    },
    {
      file: `${S}claude_terminal_launch.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "parse_background_receipt L188 parses Claude CLI stdout 'backgrounded · <handle>' into NativeClaudeJobHandle: Claude Code's direct CLI output, outside Project 2's protocols, no inventory row. Project 2 part: plan_claude_resume L209 matches SessionId and job handle by lowercase string L216-L217",
    },
    {
      file: `${S}claude_transcript_inventory.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "L57 hand-builds a storedTranscript picker record from Claude .jsonl file names (Router vocabulary, inventoried). recorded_resume_directory L81 parses each transcript line as Value and reads sessionId and cwd L110-L115: Claude Code transcripts, outside Project 2's protocols, no inventory row",
    },
    {
      file: `${S}configured_inventory_loader.rs`,
      disposition: { kind: "reads", owners: ["mcp", "router-vocabulary"] },
      note: "pages endpoints_list / sessions_list / provider_sessions_list results through ConfiguredInventoryClient and validates them (endpoint, skippedRecords, nativeCoverage, cursor bounds L340-L416); CLI SessionsSource hand-mapped to NativeSessionSource L255",
    },
    {
      file: `${S}configured_inventory_query.rs`,
      disposition: { kind: "none" },
      note: "qualify_configured_query / qualify_bound_source_filter: picker query admission against SourceInventoryBinding; no message shapes",
    },
    {
      file: `${S}configured_inventory_transport.rs`,
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "HttpConfiguredInventoryClient::read_call L119 builds an rmcp CallToolRequest L130 and POSTs it L145 with mcp-protocol-version 2025-11-25 L143; tool names endpoints_list L99, provider_sessions_list L106, sessions_list L222; decodes ServerJsonRpcMessage/CallToolResult L190-L213; Ssh variant L244/L259/L283 sends SourceOperation over SSH instead",
    },
    {
      file: `${S}native_catalogue_record.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "NativeBackgroundRecord::runtime_status L13 and native_peer_runtime_status L34 hand-map NativeClaudeBackgroundState and ClaudeCodeInteractiveStatus to the picker status",
    },
    {
      file: `${S}native_inventory_binding.rs`,
      disposition: { kind: "reads", owners: ["mcp", "router-vocabulary"] },
      note: "bind_native_inventory L41 validates an endpoints_list EndpointInventory (service, epoch, NativeCodex channel count, availability, generation) against RouterIdentity; no new shapes",
    },
    {
      file: `${S}native_inventory_pager.rs`,
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "NativeInventoryPager::next_page L67 validates sessions_list pages (endpoint, observation kind, source, generation, cursor); CollaborationClient read_native_page calls sessions_list L174",
    },
    {
      file: `${S}native_process_runner.rs`,
      disposition: { kind: "none" },
      note: "spawns the Codex CLI with argv (Command::new codex L53, L85, L117) and hands prepared sources to SSH L43; process control, no frames",
    },
    {
      file: `${S}native_summary_projection.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "from_native_summary flattens NativeSessionSummary target.session_id to a String L21 and maps runtime status L44-L49",
    },
    {
      file: `${S}picker_runtime_inventory.rs`,
      disposition: { kind: "converts", owners: ["codex", "mcp"] },
      note: "runtime_record L143 hand-reads the session_inspect thread JSON (keys L161-L208, source JSON kept as string L183); calls endpoints_list L112 and session_inspect L63; literal codex-local L115",
    },
    {
      file: `${S}provider_picker_inventory.rs`,
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "read_provider_records L26 calls endpoints_list L30/L204 and provider_sessions_list L88, validating pages (endpoint, skippedRecords, nativeCoverage, cursors)",
    },
    {
      file: `${S}router_connection_registry.rs`,
      disposition: { kind: "none" },
      note: "parses the human-owned agent-sessions.jsonc machine list (jsonc_parser L44, RouterRegistryDocument L48): private config file, not one of the four protocols",
    },
    {
      file: `${S}router_connections/connection_profile.rs`,
      disposition: { kind: "none" },
      note: "Deserialize DTOs RouterRegistryDocument L242, RouterConnectionProfileDto L249, RemoteConnectionDto L257, NativeCodexConnectionDto L281, CredentialReferenceDto L288 of the agent-sessions.jsonc config file; private config format, not one of the four protocols",
    },
    {
      file: `${S}router_connections/source_ssh_connection.rs`,
      disposition: { kind: "none" },
      note: "SshDestination / AbsoluteSourcePath / SourceClientProfile config values (serde on AbsoluteSourcePath L40, SourceClientProfile L67 for the config file and SSH request); maps SourceClientProfile to codex_native_integration::SessionProfile L75; no frames",
    },
    {
      file: `${S}session_action_selection.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "session_id L54 flattens SessionPickerIdentity (SessionRef or local String) to a String session id",
    },
    {
      file: `${S}session_catalog_query.rs`,
      disposition: { kind: "none" },
      note: "maps CLI SessionsRoot/Provider/Source/Sort to collaboration-client SessionCatalog* query enums L114-L145 for the local Codex state catalog (read in collaboration-client); private storage query, not one of the four protocols",
    },
    {
      file: `${S}session_catalog_records.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "SessionPickerIdentity L23; SessionRef rebuilt from a String L371 and flattened L507; SessionId re-parsed as UuidIdentity L181; claude-local literal L222; ClaudeCodeInteractive copied into NativeClaudePeerObservation L433",
    },
    {
      file: `${S}session_command_failures.rs`,
      disposition: { kind: "none" },
      note: "SessionsCommandError (thiserror, no serde); reads PermissionDiagnostic / overload message from NativeTransportError L133-L151 without reshaping it",
    },
    {
      file: `${S}session_command_options.rs`,
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "clap CLI parsing; SessionsSource ValueEnum L61 mirrors protocol NativeSessionSource",
    },
    {
      file: `${S}session_display_text.rs`,
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "display_title_from_session_fields L66 reads push-line headers through collaboration-protocol parse_push_line_header L76; no hand parsing",
    },
    {
      file: `${S}session_launch_selection.rs`,
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "builds SessionLaunch argv via codex_native_integration; resolve_public_native_for_endpoint L39 and resolve_public_native L60 call endpoints_list through collaboration-client (native_endpoint_selector.rs:48)",
    },
    {
      file: `${S}session_resume_selector.rs`,
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "resolve_session_resume_selector matches ClaudeCodeBackground job handles and session ids by lowercase string L27-L28, L116-L126; Codex ids come from the local state catalog",
    },
    {
      file: `${S}source_action_transport.rs`,
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "SSH source protocol exchanges (SourceOperation ProviderOperationShow L25, ProviderAction L83, ClientInventory L222, NativeClaude L251) and reply validation by operation id, target and binding L104-L197; not one of the six edges",
    },
    {
      file: `${S}source_client_action.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "SessionClientEffect L15 mirrors three SourceOperation variants and is hand-mapped to them L211-L235; builds SourceRequest L118, L236, L290; no frames on the six edges",
    },
    {
      file: `${S}source_inventory_failure.rs`,
      disposition: { kind: "converts", owners: ["mcp"] },
      note: "typed failure copies PublishedNativeInvalid L28 / PublishedProviderUnavailable L57; tool_error L81 reads the MCP isError body; published L121 maps codes -32602/-32050; into_client hand-builds bodies L152, L164",
    },
    {
      file: `${S}source_native_carrier.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "forward_native_streams L210: opaque relay of Codex app-server websocket frames between the local Codex TUI frontend and the Router native connection (L224 outbound, L241 inbound); frames are never parsed",
    },
    {
      file: `${S}source_native_claude.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "module of source_ssh_executor.rs (L567); validate_native_target calls endpoints_list L267 and re-parses SessionId as UuidIdentity L281; native_target_in_view calls provider_sessions_list L317; runs the Claude CLI and returns SourceNativeObservation",
    },
    {
      file: `${S}source_native_options.rs`,
      disposition: { kind: "none" },
      note: "parses Codex CLI --model / -c model_reasoning_effort argv (toml L60) into SourceNativeOptions; CLI options, not one of the four protocols",
    },
    {
      file: `${S}source_provider_actions.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "module of source_ssh_executor.rs (L564); provider_session_inspect L45/L175, conversation_create (composite) L122, conversation_load (composite) L159, conversation_resume L186, conversation_operation_wait L199, endpoints_list L93/L139/L265, provider_sessions_list L324 through collaboration-client; ProviderBindingIdentity hand-built from ChannelDescription L293",
    },
    {
      file: `${S}source_ssh_executor.rs`,
      disposition: { kind: "sends", owners: ["mcp", "codex"] },
      note: "decodes SourceRequest from argv L86 and writes SourceResponse L158; execute_read maps SourceOperation to tool calls endpoints_list L210/L235, sessions_list L246, provider_sessions_list L255, conversation_operation_show L281; prepare_source calls endpoints_list L357 and session_inspect L404 and reads thread.cwd by hand L412; NativeTransportConnection::connect L536 runs endpoints_list discovery; codex-local literal L367",
    },
    {
      file: `${S}source_ssh_protocol.rs`,
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "SSH source protocol (not one of the six edges): SourceRequest L20, SourceOperation L33, SourceReply L71, SourceResponse L288, SourceSessionAction L300, SourceRejection L418; carries collaboration-protocol MCP tool params/results unchanged",
    },
    {
      file: `${S}source_ssh_transport.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "SSH process transport: SourceRequest JSON as shell-quoted argv L39, SourceResponse read from stdout L123 (shapes in source_ssh_protocol.rs); codex-local literal check L182; not one of the six edges",
    },
    {
      file: `${S}test_support/configured_tls_fixture/README.md`,
      disposition: { kind: "none" },
      note: "fixture or asset, no message shapes",
    },
    {
      file: `${S}test_support/configured_tls_fixture/localhost-certificate.der`,
      disposition: { kind: "none" },
      note: "fixture or asset, no message shapes",
    },
    {
      file: `${S}test_support/configured_tls_fixture/localhost-certificate.pem`,
      disposition: { kind: "none" },
      note: "fixture or asset, no message shapes",
    },
    {
      file: `${S}test_support/configured_tls_fixture/public-test-key.der`,
      disposition: { kind: "none" },
      note: "fixture or asset, no message shapes",
    },
    {
      file: `${R}session_inventory.rs`,
      disposition: { kind: "none" },
      note: "module list and re-exports (mounted as sessions at lib.rs:6-7); cfg(test) modules L96-L99, L112-L114; no message shapes",
    },
    {
      file: `${R}session_picker_record_loader.rs`,
      disposition: { kind: "none" },
      note: "chooses configured vs local picker loading and wraps results in SourceInventoryResult; no message shapes",
    },
    {
      file: `${R}session_provider_family.rs`,
      disposition: { kind: "none" },
      note: "SessionProviderFamily display labels over protocol ProviderKind L10-L18; no serde, no conversion",
    },
    {
      file: `${R}source_inventory_binding.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary", "mcp"] },
      note: "SourceInventoryBinding::from_inventory L28 classifies an endpoints_list EndpointInventory: claude-local literal L51, channel kinds NativeCodex/ExternalProvider L55-L64",
    },
    {
      file: `${R}source_inventory_request.rs`,
      disposition: { kind: "none" },
      note: "picker request/result types; SourceInventoryRejection::InvalidNativeInventory L36 holds protocol NativeInventorySource/NativeInventoryInvalidReason as an error variant (no serde)",
    },
    {
      file: `${T}complete_source_reader.rs`,
      disposition: { kind: "none" },
      note: "test-support page consumer over SourceInventoryLoader results; no message shapes",
    },
    {
      file: `${T}fake_api_support.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "feature test-support stand-in MCP server: result envelope L111/L178, receipt_presentation L119, rejection body L184, hand service.json manifest L158 (service.json, outside Project 2's protocols, no inventory row), request read by pointer L218, reply written L206",
    },
    {
      file: `${T}mod.rs`,
      disposition: { kind: "none" },
      note: "test-support module list",
    },
    {
      file: `${T}native_partition_fixture.rs`,
      disposition: { kind: "converts", owners: ["mcp"] },
      note: "module of source_fault_fixture.rs (L19-L20): hand-built endpoints_list L58, JSON-RPC error L135, provider_sessions_list L144 with rows L167/L170, result envelope L163; reads request by pointer L36",
    },
    {
      file: `${T}recorded_api_listener.rs`,
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "test-only wire observer: records tools/call requests by /params/name L139 and relays them unchanged to the real API L162, relaying the reply back L179; never manufactures a reply",
    },
    {
      file: `${T}source_api_fixture.rs`,
      disposition: { kind: "converts", owners: ["mcp"] },
      note: "test-support real-API fixture: EndpointRef/EndpointDescription/SessionRef built as json! then from_value L108, L112, L133, L136, L165; Claude peer registry record by hand L314 (Claude Code's registry, outside Project 2's protocols, no inventory row); Codex state_5.sqlite threads table L100 is Codex private storage",
    },
    {
      file: `${T}source_fault_fixture.rs`,
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "test-support fault stand-in MCP server: request read by pointer L202, endpoints_list L237, sessions_list L339 with native_row L373, provider_sessions_list L353/L355, result envelope L369 returned as the reply",
    },
  ],
  sendSites: [
    {
      file: `${S}configured_inventory_transport.rs`,
      line: 145,
      edge: "mcp",
      sends:
        "tools/call request frame (rmcp CallToolRequest, id 1) POSTed over HTTP to the configured mcpUrl, for every configured-source tool below",
      owner: CRATE,
      origin: "product",
      coverage: CONFIGURED_HTTP,
    },
    {
      ...toolCall(
        "configured_inventory_transport.rs",
        99,
        "tools/call endpoints_list (configured HTTP source)",
      ),
      coverage: CONFIGURED_HTTP,
    },
    {
      ...toolCall(
        "configured_inventory_transport.rs",
        106,
        "tools/call provider_sessions_list (configured HTTP source)",
      ),
      coverage: CONFIGURED_HTTP,
    },
    {
      ...toolCall(
        "configured_inventory_transport.rs",
        222,
        "tools/call sessions_list (configured HTTP source)",
      ),
      coverage: CONFIGURED_HTTP,
    },
    toolCall("claude_launch_target.rs", 382, "tools/call provider_sessions_list"),
    toolCall("native_inventory_pager.rs", 174, "tools/call sessions_list"),
    toolCall("picker_runtime_inventory.rs", 63, "tools/call session_inspect"),
    toolCall("picker_runtime_inventory.rs", 112, "tools/call endpoints_list"),
    toolCall("provider_picker_inventory.rs", 30, "tools/call endpoints_list"),
    toolCall("provider_picker_inventory.rs", 88, "tools/call provider_sessions_list"),
    toolCall("provider_picker_inventory.rs", 204, "tools/call endpoints_list (revalidation)"),
    toolCall(
      "session_launch_selection.rs",
      39,
      "tools/call endpoints_list (resolve_public_native_for_endpoint, native_endpoint_selector.rs:48)",
    ),
    toolCall(
      "session_launch_selection.rs",
      60,
      "tools/call endpoints_list (resolve_public_native, native_endpoint_selector.rs:48)",
    ),
    toolCall("source_native_claude.rs", 267, "tools/call endpoints_list"),
    toolCall("source_native_claude.rs", 317, "tools/call provider_sessions_list"),
    toolCall("source_ssh_executor.rs", 210, "tools/call endpoints_list (ClientInventory)"),
    toolCall("source_ssh_executor.rs", 235, "tools/call endpoints_list (Endpoints)"),
    toolCall("source_ssh_executor.rs", 246, "tools/call sessions_list (NativeSessions)"),
    toolCall("source_ssh_executor.rs", 255, "tools/call provider_sessions_list (ProviderSessions)"),
    toolCall(
      "source_ssh_executor.rs",
      281,
      "tools/call conversation_operation_show (ProviderOperationShow)",
    ),
    toolCall("source_ssh_executor.rs", 357, "tools/call endpoints_list (prepare_source)"),
    toolCall("source_ssh_executor.rs", 404, "tools/call session_inspect (prepare_source)"),
    toolCall(
      "source_ssh_executor.rs",
      536,
      "tools/call endpoints_list (NativeTransportConnection::connect discovery; the upgrade sends no native message)",
    ),
    toolCall("source_provider_actions.rs", 45, "tools/call provider_session_inspect"),
    toolCall(
      "source_provider_actions.rs",
      93,
      "tools/call endpoints_list (ConversationClient::connect discovery)",
    ),
    toolCall(
      "source_provider_actions.rs",
      122,
      "tools/call conversation_create (composite, ConversationClient::create)",
    ),
    toolCall(
      "source_provider_actions.rs",
      139,
      "tools/call endpoints_list (ConversationClient::connect discovery)",
    ),
    toolCall(
      "source_provider_actions.rs",
      159,
      "tools/call conversation_load (composite, ConversationClient::load)",
    ),
    toolCall("source_provider_actions.rs", 175, "tools/call provider_session_inspect"),
    toolCall("source_provider_actions.rs", 186, "tools/call conversation_resume"),
    toolCall("source_provider_actions.rs", 199, "tools/call conversation_operation_wait"),
    toolCall("source_provider_actions.rs", 265, "tools/call endpoints_list (validate_binding)"),
    toolCall(
      "source_provider_actions.rs",
      324,
      "tools/call provider_sessions_list (stored_load_metadata)",
    ),
    {
      file: `${S}source_native_carrier.rs`,
      line: 224,
      edge: "cli-codex",
      sends: "opaque Codex app-server frame, local frontend to Router native connection",
      owner: { part: "N1" },
      origin: "product",
      coverage: noTap(
        "opaque byte relay; no node tap records frames relayed by forward_native_streams (no *node_tap* file references agent-session-inventory)",
      ),
    },
    {
      file: `${S}source_native_carrier.rs`,
      line: 241,
      edge: "cli-codex",
      sends: "opaque Codex app-server frame, Router native connection to local frontend",
      owner: { part: "N1" },
      origin: "product",
      coverage: noTap(
        "opaque byte relay; no node tap records frames relayed by forward_native_streams (no *node_tap* file references agent-session-inventory)",
      ),
    },
    {
      file: `${T}recorded_api_listener.rs`,
      line: 162,
      edge: "mcp",
      sends: "recorded tools/call request body relayed unchanged to the real collaboration API",
      owner: CRATE,
      origin: "apparatus",
      coverage: STAND_IN,
    },
    {
      file: `${T}recorded_api_listener.rs`,
      line: 179,
      edge: "mcp",
      sends: "real API reply relayed unchanged to the client",
      owner: CRATE,
      origin: "apparatus",
      coverage: STAND_IN,
    },
    {
      file: `${T}fake_api_support.rs`,
      line: 206,
      edge: "mcp",
      sends: "scripted tools/call reply (result, rejection or receipt envelope) over HTTP/1.1",
      owner: CRATE,
      origin: "apparatus",
      coverage: STAND_IN,
    },
    {
      file: `${T}source_fault_fixture.rs`,
      line: 368,
      edge: "mcp",
      sends:
        "scripted tools/call reply for endpoints_list / sessions_list / provider_sessions_list (and native_partition_fixture replies L226)",
      owner: CRATE,
      origin: "apparatus",
      coverage: STAND_IN,
    },
  ],
};
