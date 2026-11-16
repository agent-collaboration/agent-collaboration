import type { LedgerSection, SendSite } from "../schema-audit-types.ts";
// Part 2 of 2 by path order (conversation_client_command_reporting.rs to whoami_command.rs); part 1 is agent-collaboration.ts.

const AC = "crates/agent-collaboration/src/";
const CALLER_UNCOVERED: SendSite["coverage"] = {
  kind: "uncovered",
  reason:
    "Caller of a collaboration-client method: no node tap records what this caller's client sends; the N11 tap posts its own request and the N8/N10 taps use their own clients.",
};
function callerSite(file: string, line: number, edge: SendSite["edge"], sends: string): SendSite {
  return {
    file: `${AC}${file}`,
    line,
    edge,
    sends,
    owner: { crate: "agent-collaboration" },
    origin: "product",
    coverage: CALLER_UNCOVERED,
  };
}
const mcp = (file: string, line: number, sends: string): SendSite =>
  callerSite(file, line, "mcp", sends);
const acp = (file: string, line: number, sends: string): SendSite =>
  callerSite(file, line, "acp-served-n8", sends);
const native = (file: string, line: number, sends: string): SendSite =>
  callerSite(file, line, "cli-codex", sends);

export const section: LedgerSection = {
  root: "crates/agent-collaboration",
  files: [
    {
      file: "crates/agent-collaboration/src/conversation_client_command_reporting.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "#[path] child of conversation_client_commands.rs:16. Hand-built CLI error records json! L180 (callerCancelled), L203-L219 (afterCreate source detail), L225 (afterCreate), L267; ConversationOperationFailureKind to exit code L246 and ConversationStopReason to exit code L407. EndpointRef JSON L15, CodexGeneration JSON L332 and SessionRef JSON L347 decode through serde; typed ConversationRecord output L63, L288, L372.",
    },
    {
      file: "crates/agent-collaboration/src/conversation_client_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "#[path] child of conversation_commands.rs:28. Builds typed ConversationCreateInput/PromptInput/LoadInput/CancelInput for the client; hand maps CLI ConversationAccess to RouterAccess L206, L332, L661; ConversationActor (SessionRef or Identity) decoded from --from/--approver JSON L129, L172; no json! of its own.",
    },
    {
      file: "crates/agent-collaboration/src/conversation_client_lifecycle.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "#[path] child of conversation_client_commands.rs:26. Resume/close requests (ConversationResumeRequest L122, ConversationCloseRequest L223) over the typed client; hand maps ConversationAccess to RouterAccess L130; ProviderIdentity decoded from --from/--approver JSON L287, L298.",
    },
    {
      file: "crates/agent-collaboration/src/conversation_commands.rs",
      disposition: { kind: "reads", owners: ["acp", "router-extension", "router-vocabulary"] },
      note: "Prompt and settlement output: reads ACP stopReason L625 and maps ConversationEnd to ConversationTerminalReason L627; reads _meta.codexRouter.effectiveModel L655, effectiveEffort L662, idleSeconds L669, effectiveAccess L673, settingsObservation L680, effortChange L688 from the ACP prompt receipt by JSON pointer; ConversationAccess::as_str L227 hand-writes access strings into ConversationCreateRequest.access L353; OperationFailureKind to exit L452, ConversationEnd to exit L463.",
    },
    {
      file: "crates/agent-collaboration/src/conversation_create_input_validation.rs",
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: '#[path] child of conversation_commands.rs:25. requested_endpoint_id L36 parses an EndpointRef JSON argument as a raw Value and reads /endpointId by pointer L40, then compares it with the literal "codex-local" L13.',
    },
    {
      file: "crates/agent-collaboration/src/conversation_operation_commands.rs",
      disposition: { kind: "none" },
      note: "Builds typed ConversationOperationShow/Wait/ReconcileRequest L144, L152, L169 and serializes the typed responses with to_value L59-L70 for provider_conversation_commands::report_success; no hand-built shape or hand conversion.",
    },
    {
      file: "crates/agent-collaboration/src/conversation_settings_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "--setting string table to ProviderSettingName L60-L63; SessionRef L88 and ProviderIdentity L99 decoded from JSON; result envelope {kind,result} without cliVersion L162; Rejected.data decoded as ProviderSettingsFailure L181 and its kind mapped to exit L184-L193; error envelopes L195, L202.",
    },
    {
      file: "crates/agent-collaboration/src/current_session_identity.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "Resolves the caller's SessionRef from harness environment variables: SESSION_HARNESSES L16 maps CODEX_THREAD_ID, CLAUDE_CODE_SESSION_ID, CURSOR_CONVERSATION_ID to endpoint ids codex-local, claude-local, cursor-local; HarnessSessionIdentity::session_ref L114 builds the SessionRef.",
    },
    {
      file: "crates/agent-collaboration/src/details_client_composition.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "Compiled unconditionally (lib.rs:8 has no cfg); the earlier feature-gate note was wrong. Hand builds ProviderBindingIdentity from ChannelDescription::ExternalProvider fields plus inventory.service_epoch L91-L109; maps ProviderSessionState and Load/Resume capabilities to ManagedRestoreActions L134-L158; destructures typed SourceReply::ProviderInspection L124.",
    },
    {
      file: "crates/agent-collaboration/src/endpoint_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "result_envelope L12 {kind:result,cliVersion,result,serviceVersion?} and mutation_envelope L29 {result:{record,effects}} built with json!; normalize_result L47 reshapes any result by key sniffing (collection names L60-L71 into page.records, mutation keys L80-L87 into record/effects); report_failure L209 builds the shared CLI error envelope L216-L223. Endpoint listing prints typed EndpointInventory.",
    },
    {
      file: "crates/agent-collaboration/src/event_commands.rs",
      disposition: { kind: "converts", owners: ["codex", "router-vocabulary"] },
      note: 'events listen/observe NDJSON records with json!: listenerReady L200, connectionClosed L214, L217, L239, nativeMessage/providerSessionEvent L230, observationEvent L281, error records L256, L395; reads kind and event.kind "resyncRequired" from provider events L224-L229; OperationFailure to exit L249, L388; SessionRef assembled from discovered service id + ids L332.',
    },
    {
      file: "crates/agent-collaboration/src/failure_line.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "Renders DeliveryRejection as one human text line: reason labels L154 over every DeliveryRejectionReason, next steps from DeliveryNextAction L178, peer-claim list L45; MAX_FAILURE_LINE_SCALARS L5. Human prose only, no wire shape, so no inventory row.",
    },
    {
      file: "crates/agent-collaboration/src/instruction_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "clap types InstructionArguments L20, InstructionCommand L29, InstructionAction L36, TextInput L62; typed InstructionCreate/Update/ShowParams L225-L249; result envelope input json! L179; hand-built error envelopes L184, L190 (outcomeUnknown/unavailable with nextAction).",
    },
    {
      file: "crates/agent-collaboration/src/journal_read_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary", "mcp"] },
      note: "clap types JournalArguments L18, JournalCommand L27; EndpointRef assembled L100; typed results turned into Value L104, L110 then storage field read L116; MCP Rejected code -32602 and data.kind read L134-L145; error envelope json! L150.",
    },
    {
      file: "crates/agent-collaboration/src/lib.rs",
      disposition: { kind: "none" },
      note: "Module list and public command re-exports; help text. claude_terminal_test_harness is feature-gated L6-L7; session_contract_tests is cfg(test) L94-L95.",
    },
    {
      file: "crates/agent-collaboration/src/main.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "Binary entry and first-argument dispatch; hand-built invalidUsage error envelope json! L33 ({kind:error, error:{kind,stage,message,nextAction,details}}).",
    },
    {
      file: "crates/agent-collaboration/src/message_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "clap types MessageArguments L21, MessageCommand L27, InboxArguments L39, HistoryArguments L49, ReplyArguments L62, PushRecordShowArguments L86; invalidShape envelope json! L131, L135; OperationFailure.service_kind string literals matched L168-L176 and in the exit mapping L194-L219.",
    },
    {
      file: "crates/agent-collaboration/src/message_commands/message_send_reply.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "message send/reply build typed MessageSendRequest L72 and MessageReplyRequest L141; results wrapped with result_envelope L191, L244, L292; hand error envelopes json! L203, L305; PushDeliveryState to exit L361; DeliveryOutcome text labels L345.",
    },
    {
      file: "crates/agent-collaboration/src/message_commands/push_record_queries.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "show/inbox/history over typed requests (MessageShowRequest L132, PushRecordListParams L149, PushRecordHistoryParams L157); SessionRef from --with JSON L52; remote mail wrapper json! L196, L265; push_record_show_envelope L259 splices link and activityRanges into the result envelope; error envelope L248.",
    },
    {
      file: "crates/agent-collaboration/src/message_input_arguments.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "clap types DeliveryChoice L12, SendArguments L18, MessageSendArguments L50; CodexGeneration built as json! {serviceEpoch,generation} and decoded with from_value L132, L172; sender SessionRef from --from JSON L161; DeliveryChoice to MessageDelivery L186-L190; PreparedMessage L202 resolves to SavedMessage/MessageContent L218-L251.",
    },
    {
      file: "crates/agent-collaboration/src/native_session_commands.rs",
      disposition: { kind: "converts", owners: ["codex", "router-vocabulary", "mcp"] },
      note: "session inspect/rename and turn interrupt over the typed client, results turned into Value with json! L159-L205; fabricated MCP rejection {code:-32050,data:{kind:unavailable}} L200; reads previousName and kind from results for mutation effects L216, L220; Rejected.data kind/message read and kind mapped to exit L239-L257.",
    },
    {
      file: "crates/agent-collaboration/src/native_stdio_bridge.rs",
      disposition: { kind: "sends", owners: ["codex"] },
      note: "agent-collaboration native: relays Codex app-server JSON text frames between stdin/stdout and the Router-served native socket (NativeTransportConnection::connect L94, which opens the Router service root's NativeCodex channel). Checks JSON syntax only L129; send L132; recv Message::Text L144 written to stdout L150; invents no initialize.",
    },
    {
      file: "crates/agent-collaboration/src/permission_diagnostic_reporting.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "Renders overload L33-L48 and PermissionDiagnostic L61-L70 as CLI error envelopes with json!; wire_name L96 reads an enum's serde string for human text.",
    },
    {
      file: "crates/agent-collaboration/src/provider_conversation_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "Shared reporting for conversation operation commands: success through result_envelope L9; failure envelopes json! L36, L41, L72; ConversationOperationFailureKind to exit L54-L70 (same table as conversation_client_command_reporting.rs L246).",
    },
    {
      file: "crates/agent-collaboration/src/question_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "clap types QuestionArguments L19, QuestionCommand L25, OutputArguments L51; typed QuestionAnswerParams L114 with QuestionAnswerValue map decoded from --content JSON L88; OperationFailureKind to exit L194-L201; error envelope json! L206.",
    },
    {
      file: "crates/agent-collaboration/src/run_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "clap types RunArguments L16, RunCommand L25, RunAction L39, PreparedRun L60; result envelope input json! L178; LocalMutationEvidence to exit L183-L191; hand error envelopes L190, L197.",
    },
    {
      file: "crates/agent-collaboration/src/schedule_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "clap types ScheduleArguments L21, ScheduleCommand L30, ScheduleAction L37; PreparedSchedule L94; ScheduleOutput L120 untagged Serialize union of ScheduleSnapshot and ScheduleExportResult; result envelope input json! L262; DeliveryRouteEvidence effects (CodexAppServer, ProviderAcp, ClaudeCodePeer) classified as uncertain exit L267-L281; error envelopes L280, L287; ScheduleDefinition JSON decoded L392.",
    },
    {
      file: "crates/agent-collaboration/src/schedule_preparation_arguments.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "Builds DestinationPreparation from flags: Fresh with EndpointRef assembled from the discovered service id L37-L45; Existing/Fork targets decoded as SessionRef JSON L56, L66. Typed construction and serde decode only, no inventory row.",
    },
    {
      file: "crates/agent-collaboration/src/session_client_composition.rs",
      disposition: { kind: "none" },
      note: "Compiled unconditionally (lib.rs:9 has no cfg); the earlier feature-gate note was wrong. Composes session-picker ports from agent-session-inventory functions and destructures the typed SourceReply::ClientInventory L89; builds no message shape.",
    },
    {
      file: "crates/agent-collaboration/src/session_command_dispatch.rs",
      disposition: { kind: "none" },
      note: "Sessions catalog listing and Codex CLI launch dispatch: typed session records written with serde L218; Codex launch arguments (not a protocol message) L405-L440; picker request assembly L270-L302.",
    },
    {
      file: "crates/agent-collaboration/src/session_commands/stored_model_terminal_fixture.rs",
      disposition: { kind: "none" },
      note: "Behind the quota-reset-test-harness feature (declared at session_command_dispatch.rs:513-515): sealed picker fixture with a stand-in Codex state_5.sqlite threads table; no message shapes.",
    },
    {
      file: "crates/agent-collaboration/src/session_contract_tests/session_test_support.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at session_contract_tests.rs:10 inside session_contract_tests, which lib.rs:94-95 declares #[cfg(test)]",
      },
    },
    {
      file: "crates/agent-collaboration/src/session_environment.rs",
      disposition: { kind: "none" },
      note: "CliContext environment access, app-server socket and service-directory selection, Codex home and session profile; no message shapes.",
    },
    {
      file: "crates/agent-collaboration/src/session_inventory_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary", "mcp"] },
      note: "classify_endpoint_for_session_list L44 forces the literal claude-local to Provider L45; CLI InventoryView/InventorySource to NativeSessionView/NativeSessionSource L172-L201; fabricated MCP rejections {code:-32050,data:{kind,stage,message}} L206, L221; code -32602 and data.kind read for exit L242-L253; error envelope json! L258.",
    },
    {
      file: "crates/agent-collaboration/src/session_target_arguments.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "One grammar for session targets: --to decoded as SessionRef JSON L29; into_mail_recipient L58 turns an (endpoint id, session id) pair plus the local service id into a SessionRef, used by message, wake, native session and conversation commands.",
    },
    {
      file: "crates/agent-collaboration/src/wakeup_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "MessageContent to WakeCreator L168-L181; result envelope input json! L219; LocalMutationEvidence to exit L224-L232; reads /result/record/definition/wakeupId back out of its own envelope and decodes WakeupId L245-L249; WakeWaitFailureKind to exit L263-L272; firstFire spliced into the envelope L307; hand error envelopes L231, L238, L274, L282, L312.",
    },
    {
      file: "crates/agent-collaboration/src/wakeup_timing_arguments.rs",
      disposition: { kind: "none" },
      note: "Builds typed TimingRequest/ExpiryRequest from timer flags L29-L66 and parses durations L68; no hand-built shape.",
    },
    {
      file: "crates/agent-collaboration/src/wakeup_wait_output.rs",
      disposition: { kind: "none" },
      note: "Waits for the first fire through the typed client (subscribe_wakeup L17) and returns the typed FireReceipt.",
    },
    {
      file: "crates/agent-collaboration/src/whoami_command.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "whoami record json! L142 {session, source, endpointRegistered, machineId (service id), peerId, machineLabel}; SessionRef from harness identity L116.",
    },
  ],
  sendSites: [
    mcp(
      "conversation_client_command_reporting.rs",
      46,
      "CollaborationClient::list_endpoints -> tools/call endpoints_list",
    ),
    mcp(
      "conversation_client_commands.rs",
      220,
      "ConversationClient::connect -> endpoint discovery tools/call endpoints_list",
    ),
    acp(
      "conversation_client_commands.rs",
      220,
      "ConversationClient::connect (Codex endpoint) -> AcpConversation::connect_with_context initialize",
    ),
    acp(
      "conversation_client_commands.rs",
      226,
      "ConversationClient::create (CodexAcp) -> session/new",
    ),
    mcp(
      "conversation_client_commands.rs",
      226,
      "ConversationClient::create (ExternalProvider) -> tools/call conversation_create",
    ),
    acp(
      "conversation_client_commands.rs",
      399,
      "ConversationClient::create_and_prompt (CodexAcp) -> initialize, session/new, session/prompt",
    ),
    mcp(
      "conversation_client_commands.rs",
      399,
      "ConversationClient::create_and_prompt -> tools/call endpoints_list; ExternalProvider: conversation_create, conversation_prompt",
    ),
    mcp(
      "conversation_client_commands.rs",
      528,
      "ConversationClient::connect -> endpoint discovery tools/call endpoints_list",
    ),
    acp(
      "conversation_client_commands.rs",
      528,
      "ConversationClient::connect (Codex endpoint) -> AcpConversation::connect_with_context initialize",
    ),
    acp(
      "conversation_client_commands.rs",
      561,
      "ConversationClient::prompt (CodexAcp) -> session/load, session/prompt",
    ),
    mcp(
      "conversation_client_commands.rs",
      561,
      "ConversationClient::prompt (ExternalProvider) -> tools/call conversation_prompt",
    ),
    mcp(
      "conversation_client_commands.rs",
      639,
      "ConversationClient::connect -> endpoint discovery tools/call endpoints_list",
    ),
    acp(
      "conversation_client_commands.rs",
      639,
      "ConversationClient::connect (Codex endpoint) -> AcpConversation::connect_with_context initialize",
    ),
    acp(
      "conversation_client_commands.rs",
      668,
      "ConversationClient::load (CodexAcp) -> session/load",
    ),
    mcp(
      "conversation_client_commands.rs",
      668,
      "ConversationClient::load (ExternalProvider) -> tools/call conversation_load",
    ),
    mcp(
      "conversation_client_commands.rs",
      744,
      "ConversationClient::connect -> endpoint discovery tools/call endpoints_list",
    ),
    acp(
      "conversation_client_commands.rs",
      744,
      "ConversationClient::connect (Codex endpoint) -> AcpConversation::connect_with_context initialize",
    ),
    mcp(
      "conversation_client_commands.rs",
      758,
      "ConversationClient::cancel (ExternalProvider; CodexAcp refuses before sending) -> tools/call conversation_cancel",
    ),
    mcp(
      "conversation_client_lifecycle.rs",
      122,
      "CollaborationClient::resume_provider_conversation -> tools/call conversation_resume",
    ),
    mcp(
      "conversation_client_lifecycle.rs",
      223,
      "CollaborationClient::close_provider_conversation -> tools/call conversation_close",
    ),
    mcp(
      "conversation_client_lifecycle.rs",
      257,
      "CollaborationClient::wait_for_provider_conversation_operation -> tools/call conversation_operation_wait",
    ),
    mcp(
      "conversation_commands.rs",
      311,
      "AcpConversation::connect_with_context -> AcpTransportConnection endpoint discovery tools/call endpoints_list",
    ),
    acp("conversation_commands.rs", 311, "AcpConversation::connect_with_context -> initialize"),
    acp(
      "conversation_commands.rs",
      377,
      "AcpConversation::open_and_prompt -> session/new or session/load (fork/new/load), then session/prompt",
    ),
    mcp(
      "conversation_operation_commands.rs",
      59,
      "CollaborationClient::show_provider_conversation_operation -> tools/call conversation_operation_show",
    ),
    mcp(
      "conversation_operation_commands.rs",
      63,
      "CollaborationClient::wait_for_provider_conversation_operation -> tools/call conversation_operation_wait",
    ),
    mcp(
      "conversation_operation_commands.rs",
      68,
      "CollaborationClient::reconcile_provider_conversation_operation -> tools/call conversation_operation_reconcile",
    ),
    mcp(
      "conversation_settings_commands.rs",
      142,
      "CollaborationClient::set_provider_conversation_setting -> tools/call conversation_settings_set",
    ),
    mcp(
      "conversation_settings_commands.rs",
      152,
      "CollaborationClient::accept_provider_conversation_settings -> tools/call conversation_settings_accept",
    ),
    mcp(
      "endpoint_commands.rs",
      151,
      "CollaborationClient::list_endpoints -> tools/call endpoints_list",
    ),
    mcp(
      "event_commands.rs",
      171,
      "SessionObservation::attach_by_ids_with_context -> endpoint discovery tools/call endpoints_list",
    ),
    native(
      "event_commands.rs",
      171,
      "SessionObservation::attach_by_ids_with_context -> native initialize and thread/resume on the Router native socket",
    ),
    mcp(
      "event_commands.rs",
      171,
      "SessionObservation::attach_by_ids_with_context, provider endpoint branch -> tools/call events_observe for the first snapshot and each follow-up observation (collaboration-client provider_session_observation.rs:70-111,345)",
    ),
    mcp(
      "event_commands.rs",
      343,
      "SessionObservation::observe_bounded -> endpoint discovery tools/call endpoints_list",
    ),
    native(
      "event_commands.rs",
      343,
      "SessionObservation::observe_bounded -> native initialize and thread/resume on the Router native socket",
    ),
    mcp(
      "event_commands.rs",
      343,
      "SessionObservation::observe_bounded, provider endpoint branch -> tools/call events_observe (collaboration-client provider_session_observation.rs:128-154,345)",
    ),
    mcp(
      "instruction_commands.rs",
      159,
      "CollaborationClient::create_instruction -> tools/call instruction_create",
    ),
    mcp(
      "instruction_commands.rs",
      160,
      "CollaborationClient::update_instruction -> tools/call instruction_update",
    ),
    mcp(
      "instruction_commands.rs",
      161,
      "CollaborationClient::read_instruction -> tools/call instruction_show",
    ),
    mcp(
      "journal_read_commands.rs",
      106,
      "CollaborationClient::read_journal -> tools/call journal_read",
    ),
    mcp(
      "journal_read_commands.rs",
      110,
      "CollaborationClient::journal_status -> tools/call journal_status",
    ),
    mcp(
      "message_commands/message_send_reply.rs",
      82,
      "CollaborationClient::send_message -> tools/call message_send",
    ),
    mcp(
      "message_commands/message_send_reply.rs",
      141,
      "CollaborationClient::reply_to_push -> tools/call message_reply",
    ),
    mcp(
      "message_commands/push_record_queries.rs",
      132,
      "CollaborationClient::router_show -> tools/call router_show",
    ),
    mcp(
      "message_commands/push_record_queries.rs",
      149,
      "CollaborationClient::message_inbox -> tools/call message_inbox",
    ),
    mcp(
      "message_commands/push_record_queries.rs",
      157,
      "CollaborationClient::message_history -> tools/call message_history",
    ),
    mcp(
      "native_session_commands.rs",
      148,
      "CollaborationClient::list_endpoints -> tools/call endpoints_list",
    ),
    mcp(
      "native_session_commands.rs",
      161,
      "CollaborationClient::inspect_provider_session -> tools/call provider_session_inspect",
    ),
    mcp(
      "native_session_commands.rs",
      169,
      "CollaborationClient::inspect_session -> tools/call session_inspect",
    ),
    mcp(
      "native_session_commands.rs",
      176,
      "CollaborationClient::rename_session -> tools/call session_rename",
    ),
    mcp(
      "native_session_commands.rs",
      184,
      "CollaborationClient::list_endpoints -> tools/call endpoints_list",
    ),
    mcp(
      "native_session_commands.rs",
      205,
      "CollaborationClient::interrupt_turn -> tools/call turn_interrupt",
    ),
    mcp(
      "native_stdio_bridge.rs",
      94,
      "NativeTransportConnection::connect -> endpoint discovery tools/call endpoints_list",
    ),
    {
      file: "crates/agent-collaboration/src/native_stdio_bridge.rs",
      line: 132,
      edge: "cli-codex",
      sends:
        "Codex app-server JSON text frames relayed unparsed from stdin to the Router native connection",
      owner: { crate: "agent-collaboration" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "No node tap records frames relayed by this bridge; no *node_tap* file references native_stdio_bridge or run_native_command.",
      },
    },
    {
      file: "crates/agent-collaboration/src/native_stdio_bridge.rs",
      line: 150,
      edge: "cli-codex",
      sends:
        "Codex app-server JSON text frames from the Router native connection written as JSON lines to stdout (the Codex client side)",
      owner: { crate: "agent-collaboration" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "No node tap records frames relayed by this bridge; no *node_tap* file references native_stdio_bridge or run_native_command.",
      },
    },
    mcp(
      "question_commands.rs",
      156,
      "CollaborationClient::answer_question -> tools/call question_answer",
    ),
    mcp(
      "question_commands.rs",
      161,
      "CollaborationClient::list_questions -> tools/call question_list",
    ),
    mcp("run_commands.rs", 158, "CollaborationClient::read_run -> tools/call run_show"),
    mcp(
      "run_commands.rs",
      159,
      "CollaborationClient::retry_summary -> tools/call run_summary_retry",
    ),
    mcp("run_commands.rs", 160, "CollaborationClient::skip_summary -> tools/call run_summary_skip"),
    mcp(
      "schedule_commands.rs",
      200,
      "CollaborationClient::import_schedule -> tools/call schedule_import",
    ),
    mcp(
      "schedule_commands.rs",
      203,
      "CollaborationClient::export_schedule -> tools/call schedule_export",
    ),
    mcp(
      "schedule_commands.rs",
      211,
      "CollaborationClient::prepare_schedule -> tools/call schedule_prepare",
    ),
    mcp(
      "schedule_commands.rs",
      219,
      "CollaborationClient::create_schedule -> tools/call schedule_create",
    ),
    mcp(
      "schedule_commands.rs",
      222,
      "CollaborationClient::update_schedule -> tools/call schedule_update",
    ),
    mcp(
      "schedule_commands.rs",
      225,
      "CollaborationClient::read_schedule -> tools/call schedule_show",
    ),
    mcp(
      "schedule_commands.rs",
      228,
      "CollaborationClient::enable_schedule -> tools/call schedule_enable",
    ),
    mcp(
      "schedule_commands.rs",
      231,
      "CollaborationClient::disable_schedule -> tools/call schedule_disable",
    ),
    mcp(
      "session_inventory_commands.rs",
      202,
      "CollaborationClient::list_endpoints -> tools/call endpoints_list",
    ),
    mcp(
      "session_inventory_commands.rs",
      213,
      "CollaborationClient::list_sessions -> tools/call sessions_list",
    ),
    mcp(
      "session_inventory_commands.rs",
      217,
      "CollaborationClient::list_provider_sessions -> tools/call provider_sessions_list",
    ),
    mcp("wakeup_commands.rs", 166, "CollaborationClient::send_wakeup -> tools/call wake_send"),
    mcp("wakeup_commands.rs", 191, "CollaborationClient::read_wakeup -> tools/call wake_show"),
    mcp("wakeup_commands.rs", 194, "CollaborationClient::list_wakeups -> tools/call wake_list"),
    mcp("wakeup_commands.rs", 197, "CollaborationClient::pause_wakeup -> tools/call wake_pause"),
    mcp("wakeup_commands.rs", 198, "CollaborationClient::resume_wakeup -> tools/call wake_resume"),
    mcp("wakeup_commands.rs", 199, "CollaborationClient::cancel_wakeup -> tools/call wake_cancel"),
    mcp(
      "wakeup_wait_output.rs",
      17,
      "CollaborationClient::subscribe_wakeup -> tools/call wake_show",
    ),
    mcp(
      "wakeup_wait_output.rs",
      19,
      "WakeWaitConnection::wait_until_first_fire -> tools/call wake_wait_until_first_fire",
    ),
    mcp(
      "whoami_command.rs",
      109,
      "CollaborationClient::list_endpoints -> tools/call endpoints_list",
    ),
    mcp(
      "whoami_command.rs",
      113,
      "CollaborationClient::installation_show -> tools/call installation_show",
    ),
  ],
};
