import type { LedgerSection, SendSite } from "../schema-audit-types.ts";

const R = "crates/codex-router-debug-hosts/src/bin/";
const owner = { crate: "codex-router-debug-hosts" };
const live: SendSite["coverage"] = {
  kind: "uncovered",
  reason: "live acceptance harness against an owned debug Host; no node tap records it",
};
const liveMcp: SendSite["coverage"] = {
  kind: "uncovered",
  reason:
    "live acceptance harness caller of CollaborationClient; the tools/call is posted at collaboration-client api_connection.rs:141 and no node tap records this caller",
};

/** A managed-codex send from this harness, uncovered by any tap. */
function codexSend(file: string, line: number, sends: string): SendSite {
  return {
    file: `${R}${file}`,
    line,
    edge: "managed-codex",
    sends,
    owner,
    origin: "apparatus",
    coverage: live,
  };
}

/** NativeProtocolConnection::connect at a caller: initialize then initialized (codex-native-integration native_protocol_connection.rs:100,104). */
function connectSends(file: string, line: number): SendSite[] {
  return [
    codexSend(file, line, "initialize via NativeProtocolConnection::connect"),
    codexSend(file, line, "initialized notification via NativeProtocolConnection::connect"),
  ];
}

/** inspect_thread at a caller: thread/read {threadId,includeTurns:false} (codex-native-integration native_thread_operations.rs:32-35). */
function inspectSend(file: string, line: number): SendSite {
  return codexSend(file, line, "thread/read {threadId,includeTurns:false} via inspect_thread");
}

/** A CollaborationClient tool call at a caller. */
function mcpCall(file: string, line: number, tool: string): SendSite {
  return {
    file: `${R}${file}`,
    line,
    edge: "mcp",
    sends: `tools/call ${tool} via CollaborationClient`,
    owner,
    origin: "apparatus",
    coverage: liveMcp,
  };
}

export const section: LedgerSection = {
  root: "crates/codex-router-debug-hosts",
  files: [
    {
      file: `${R}automation-debug-host.rs`,
      disposition: { kind: "none" },
      note: "acceptance Host launcher; DebugHostContext serde (37-53) is the harness's own run-directory marker file (debug-host-context.json, 328-438), not a message; writes a TOML profile (216-223) and starts the Host",
    },
    {
      file: `${R}automation_debug_host/host_replacement.rs`,
      disposition: { kind: "none" },
      note: "re-exec command and inherited Host lock acquisition, no messages",
    },
    {
      file: `${R}automation_debug_host/launch_options.rs`,
      disposition: { kind: "none" },
      note: "debug launch option parsing; ProviderCommand serde (12-17) is persisted inside the harness context marker, not a message; projects Router and provider child commands (257-317)",
    },
    {
      file: `${R}automation_debug_host/proof_permissions.rs`,
      disposition: { kind: "none" },
      note: "builds Codex `-c permissions.automation-proof=` and `-c features.network_proxy=` TOML command-line overrides (24-67) for the app-server process; process arguments, not a wire message",
    },
    {
      file: `${R}automation_debug_host/startup_diagnostics.rs`,
      disposition: { kind: "none" },
      note: "tracing JSON layer for startup diagnostics, no message shapes",
    },
    {
      file: `${R}board-debug-runtime.rs`,
      disposition: { kind: "none" },
      note: "board listeners without a native process; reads service.json with the owning ServiceManifest type (105-106); json! status lines to stdout (122,129) are harness output",
    },
    {
      file: `${R}board-debug-runtime/fixture_provider.rs`,
      disposition: { kind: "none" },
      note: "FixtureProviderLaunch serde (8-13) reads the journey's fixture-provider-acp.json launch file, not a message",
    },
    {
      file: `${R}board-debug-runtime/local_cancel_observation.rs`,
      disposition: { kind: "none" },
      note: "test-only stdin query protocol ProjectQuery serde (9-17) and json! reply lines (40-81); debug harness IPC, not one of the four protocols",
    },
    {
      file: `${R}board-debug-runtime/mail_faults.rs`,
      disposition: { kind: "none" },
      note: "decodes the debug fault file into interaction_storage::MailStoreTestFault (12-13), its owning type",
    },
    {
      file: `${R}board-debug-runtime/shutdown_signals.rs`,
      disposition: { kind: "none" },
      note: "signal registration",
    },
    {
      file: `${R}central_sign_in_launcher.rs`,
      disposition: { kind: "none" },
      note: "dev-only sign-in fixture; Readiness serde line (24-43) on stdout and OrganizationSignInLink over a private socket (62); central design fixture, not one of the six edges",
    },
    {
      file: `${R}central_sign_in_launcher/command_protocol.rs`,
      disposition: { kind: "none" },
      note: "closed dev fixture stdin commands LauncherCommand and stderr answers LauncherAnswer (serde, 12-45); fixture IPC, not one of the four protocols",
    },
    {
      file: `${R}central_sign_in_launcher/launch_arguments.rs`,
      disposition: { kind: "none" },
      note: "flag parsing",
    },
    {
      file: `${R}central_sign_in_launcher/launcher_failure.rs`,
      disposition: { kind: "none" },
      note: "safe diagnostics",
    },
    {
      file: `${R}central_sign_in_launcher/shutdown_signals.rs`,
      disposition: { kind: "none" },
      note: "signal registration",
    },
    {
      file: `${R}central_sign_in_launcher/stdin_commands.rs`,
      disposition: { kind: "none" },
      note: "stdin line reader with cancellable poll",
    },
    {
      file: `${R}debug_host_acceptance.rs`,
      disposition: { kind: "sends", owners: ["codex", "mcp", "router-vocabulary"] },
      note: "acceptance driver: NativeProtocolConnection::connect (271,351) sends initialize and initialized; sends config/read {includeLayers:false} (273) and remoteControl/status/read (290) and reads config.model_provider and model_providers base_url, status (275-293); CollaborationClient endpoints_list calls (237,396,440); builds collaboration_protocol::SessionRef from native thread id strings by try_into (403-410, 447-454); json! at 171-501 are harness result lines",
    },
    {
      file: `${R}debug_host_acceptance/acp_client_proof.rs`,
      disposition: { kind: "sends", owners: ["codex", "acp"] },
      note: "inspect_thread (38,83) sends thread/read {includeTurns:false} and reads thread.model; parses agent-collaboration `conversation prompt --json` record lines sessionReady, sessionUpdate update.sessionUpdate agent_message_chunk update.content.text, promptResult result.stopReason end_turn (130-156); the ACP client in scripts/proof-tools/acp-client-proof.mjs and the CLI do their own sends outside this crate",
    },
    {
      file: `${R}debug_host_acceptance/acp_permission_proof.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "answers the Codex server request item/commandExecution/requestApproval with {decision:cancel} (153) through submit_callback_response (148); reads params.threadId, command, availableDecisions, itemId by Value::get (119-147) and item/completed item.type commandExecution, item.status declined, item.id (159-176); reads the proof script receipt kind, sessionId, toolCallId (90-94)",
    },
    {
      file: `${R}debug_host_acceptance/acp_replacement_proof.rs`,
      disposition: { kind: "sends", owners: ["codex", "mcp"] },
      note: "NativeProtocolConnection::connect (120) sends initialize and initialized; inspect_thread (121) sends thread/read {includeTurns:false} and reads thread.model; CollaborationClient endpoints_list (163) read as typed ChannelDescription::NativeCodex generation; proof script record lines read by kind and sessionId (142-158), harness output",
    },
    {
      file: `${R}debug_host_acceptance/agent_tool_proof.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary", "codex"] },
      note: "serializes collaboration SessionRef values to JSON text with serde_json::to_string (19-20) and embeds them as CLI --from/--to arguments in the model's task prompt (21-35); reads Codex notifications by Value::get: request id presence (57), item/completed item.type commandExecution, command, exitCode, aggregatedOutput, agentMessage text (58-80)",
    },
    {
      file: `${R}debug_host_acceptance/cli_event_listener.rs`,
      disposition: { kind: "reads", owners: ["router-vocabulary", "codex"] },
      note: "hand-parses agent-collaboration `events listen` record lines: listenerReady target.sessionId and generation (44-53), nativeMessage generation, target.sessionId and the wrapped Codex message, callback detection by method+id (79-92); run_json_command reads the CLI result envelope result (122-133)",
    },
    {
      file: `${R}debug_host_acceptance/cli_message_proof.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "SessionRef to JSON text for CLI --to/--from arguments (18,20) and serde_json::to_value comparison with the receipt (39); reads the CLI receipt result.target, acceptance.kind nativeInputAccepted, acceptance.turnId and error (30-47)",
    },
    {
      file: `${R}debug_host_acceptance/lifecycle_reader_proof.rs`,
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "CollaborationClient tool calls journal_status (21), sessions_list (48), addresses_list (75), journal_read (111,120), decoded into the typed collaboration_protocol shapes; serde_json::to_value of watermark and coverage (79-80) only to compare pages, not a conversion between types",
    },
    {
      file: `${R}debug_host_acceptance/native_delivery_proof.rs`,
      disposition: { kind: "sends", owners: ["codex", "router-vocabulary"] },
      note: "sends thread/queue/delete {threadId,queuedSubmissionId} (146), thread/queue/list {threadId,limit} (274), thread/read {threadId,includeTurns:true} (294) and inspect_thread thread/read (138); hand-built SessionRef JSON {endpoint:{serviceId,endpointId},sessionId} (47-50) passed as CLI --from/--to text (223,225) and compared with receipts (244); reads CLI receipts acceptance.kind/turnId/submissionId, generation serviceEpoch/generation, interruptCompleted (61-136, 233-262) and Codex notifications turn/completed turn.id/status, item/completed agentMessage text, item/agentMessage/delta turnId, queue data[].id, thread.turns[].status, status.type (63, 115-127, 139, 174-198, 276-307)",
    },
    {
      file: `${R}debug_host_acceptance/native_fork_proof.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "sends thread/read {threadId,includeTurns:true} twice (25,32) around a fork and compares thread values",
    },
    {
      file: `${R}debug_host_acceptance/owned_thread_registry.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "sends thread/fork {threadId,cwd,deferGoalContinuation} (28), thread/start with a hand-built params object (103-118) including a hand-built permissions/features.network_proxy config object (77-92), inspect_thread thread/read (183) and turn/start {threadId,model,effort,input:[{type:text,text}]} (195); reads reply fields thread.id, modelProvider, model, reasoningEffort, sandbox.type, approvalPolicy, approvalsReviewer, cwd, turn.id (33-51, 139-164, 200-204) and notifications error, item/agentMessage/delta, item/completed, turn/completed (243-309)",
    },
    {
      file: `${R}debug_host_acceptance/owned_turn_error_diagnostic.rs`,
      disposition: { kind: "reads", owners: ["codex"] },
      note: "hand classification tables of Codex codexErrorInfo variants (STRING_CLASSES 5-21, STATUS_CLASSES 22-27) and reader of error.codexErrorInfo, *.httpStatusCode and willRetry (39-68)",
    },
    {
      file: `${R}debug_host_acceptance/process_identity_guard.rs`,
      disposition: { kind: "none" },
      note: "read-only lsof/ps process identity snapshots",
    },
    {
      file: `${R}debug_host_acceptance/proof_environment_settings.rs`,
      disposition: { kind: "converts", owners: ["codex"] },
      note: "adds features.hooks:false to the thread config overrides object passed in thread/start (13-20)",
    },
    {
      file: `${R}debug_host_acceptance/recovery_observation.rs`,
      disposition: { kind: "sends", owners: ["codex", "mcp", "router-vocabulary"] },
      note: "probe NativeProtocolConnection::connect (44) sends initialize and initialized, then config/read (45) and inspect_thread thread/read (46); CollaborationClient endpoints_list (18,33); re-serializes the typed endpoint inventory (decoded by CollaborationClient::list_endpoints) with json! and reads it back by pointer endpoint.endpointId and availability.state (36-41): a local view of the canonical decoded type (boundary-encoding row); json! lines are harness output",
    },
    {
      file: `${R}imagegen-debug-acceptance.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "NativeProtocolConnection::connect to the owned app-server socket (265) sends initialize and initialized; the proof requests are in imagegen_debug_acceptance/native_proof.rs; AcceptanceSummary and IsolationReceipt serde (37-67) are harness artifact files",
    },
    {
      file: `${R}imagegen_debug_acceptance/audit_proof.rs`,
      disposition: { kind: "none" },
      note: "reads Router audit JSON lines route_kind, outcome, account_hash (41-58), an audit file not a message; json! at 73 builds a redacted evidence file, harness output",
    },
    {
      file: `${R}imagegen_debug_acceptance/native_proof.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "sends modelProvider/capabilities/read (41), config/read (99), remoteControl/status/read (118), model/list {cursor,limit,includeHidden} (132), thread/start (171), turn/start (211) to the owned app-server; reads replies by Value::get (43-45, 101-121, 137-159, 187-201, 220-225) and notifications item/started and item/completed imageGeneration status/result/savedPath, turn/completed turn.id/status (237-303)",
    },
    {
      file: `${R}imagegen_debug_acceptance/profile.rs`,
      disposition: { kind: "none" },
      note: "TOML profile projection (20-56), a config file not a message",
    },
    {
      file: `${R}provider-only-debug-host.rs`,
      disposition: { kind: "none" },
      note: "provider-only proof Host launcher; Claude provider environment (65-74); json! status line at 115 is harness output",
    },
  ],
  sendSites: [
    ...connectSends("debug_host_acceptance.rs", 271),
    codexSend("debug_host_acceptance.rs", 273, "config/read {includeLayers:false}"),
    codexSend("debug_host_acceptance.rs", 290, "remoteControl/status/read {}"),
    ...connectSends("debug_host_acceptance.rs", 351),
    mcpCall("debug_host_acceptance.rs", 237, "endpoints_list"),
    mcpCall("debug_host_acceptance.rs", 396, "endpoints_list"),
    mcpCall("debug_host_acceptance.rs", 440, "endpoints_list"),
    inspectSend("debug_host_acceptance/acp_client_proof.rs", 38),
    inspectSend("debug_host_acceptance/acp_client_proof.rs", 83),
    codexSend(
      "debug_host_acceptance/acp_permission_proof.rs",
      153,
      "response to item/commandExecution/requestApproval {decision:cancel}",
    ),
    ...connectSends("debug_host_acceptance/acp_replacement_proof.rs", 120),
    inspectSend("debug_host_acceptance/acp_replacement_proof.rs", 121),
    mcpCall("debug_host_acceptance/acp_replacement_proof.rs", 163, "endpoints_list"),
    mcpCall("debug_host_acceptance/lifecycle_reader_proof.rs", 21, "journal_status"),
    mcpCall("debug_host_acceptance/lifecycle_reader_proof.rs", 48, "sessions_list"),
    mcpCall("debug_host_acceptance/lifecycle_reader_proof.rs", 75, "addresses_list"),
    mcpCall("debug_host_acceptance/lifecycle_reader_proof.rs", 111, "journal_read"),
    mcpCall("debug_host_acceptance/lifecycle_reader_proof.rs", 120, "journal_read"),
    inspectSend("debug_host_acceptance/native_delivery_proof.rs", 138),
    codexSend(
      "debug_host_acceptance/native_delivery_proof.rs",
      146,
      "thread/queue/delete {threadId,queuedSubmissionId}",
    ),
    codexSend(
      "debug_host_acceptance/native_delivery_proof.rs",
      274,
      "thread/queue/list {threadId,limit}",
    ),
    codexSend(
      "debug_host_acceptance/native_delivery_proof.rs",
      294,
      "thread/read {threadId,includeTurns}",
    ),
    codexSend(
      "debug_host_acceptance/native_fork_proof.rs",
      25,
      "thread/read {threadId,includeTurns:true}",
    ),
    codexSend(
      "debug_host_acceptance/native_fork_proof.rs",
      32,
      "thread/read {threadId,includeTurns:true}",
    ),
    codexSend(
      "debug_host_acceptance/owned_thread_registry.rs",
      28,
      "thread/fork {threadId,cwd,deferGoalContinuation}",
    ),
    codexSend(
      "debug_host_acceptance/owned_thread_registry.rs",
      118,
      "thread/start (hand-built params object)",
    ),
    inspectSend("debug_host_acceptance/owned_thread_registry.rs", 183),
    codexSend(
      "debug_host_acceptance/owned_thread_registry.rs",
      195,
      "turn/start {threadId,model,effort,input}",
    ),
    mcpCall("debug_host_acceptance/recovery_observation.rs", 18, "endpoints_list"),
    mcpCall("debug_host_acceptance/recovery_observation.rs", 33, "endpoints_list"),
    ...connectSends("debug_host_acceptance/recovery_observation.rs", 44),
    codexSend(
      "debug_host_acceptance/recovery_observation.rs",
      45,
      "config/read {includeLayers:false}",
    ),
    inspectSend("debug_host_acceptance/recovery_observation.rs", 46),
    ...connectSends("imagegen-debug-acceptance.rs", 265),
    codexSend(
      "imagegen_debug_acceptance/native_proof.rs",
      41,
      "modelProvider/capabilities/read {}",
    ),
    codexSend("imagegen_debug_acceptance/native_proof.rs", 99, "config/read {includeLayers:false}"),
    codexSend("imagegen_debug_acceptance/native_proof.rs", 118, "remoteControl/status/read {}"),
    codexSend(
      "imagegen_debug_acceptance/native_proof.rs",
      132,
      "model/list {cursor,limit,includeHidden}",
    ),
    codexSend(
      "imagegen_debug_acceptance/native_proof.rs",
      171,
      "thread/start (hand-built params object)",
    ),
    codexSend(
      "imagegen_debug_acceptance/native_proof.rs",
      211,
      "turn/start {threadId,model,input}",
    ),
  ],
};
