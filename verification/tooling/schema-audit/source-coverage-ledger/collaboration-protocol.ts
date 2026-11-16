import type { LedgerSection } from "../schema-audit-types.ts";
// Left out by name only: 3 colocated unit-test files (*_test.rs, *_tests.rs):
// message_send_result_tests.rs (declared message_send_result.rs:35-37), push_line_parse_tests.rs
// (push_line_parse.rs:44-46) and push_line_tests.rs (lib.rs:119-121). No file in this crate
// performs I/O, so it has no send sites.

export const section: LedgerSection = {
  root: "crates/collaboration-protocol",
  files: [
    {
      file: "crates/collaboration-protocol/src/access_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "codex", "mcp"] },
      note: "RouterAccess L7, SettingsObservationSource L16 (threadStart/threadFork/threadResume: which Codex reply the settings came from), SettingsUnavailableReason L24, SettingsObservation L41 whose Observed variant carries Codex nativeSandbox, permissionProfile, approvalPolicy and approvalsReviewer as untyped serde_json::Value (L46-L49).",
    },
    {
      file: "crates/collaboration-protocol/src/acp_schema_catalog.rs",
      disposition: { kind: "defines", owners: ["acp"] },
      note: "Vendored ACP JSON Schema: ACP_SCHEMA_BYTES L6 (include_bytes), ACP_SCHEMA_DIGEST L8, AcpSchemaCatalog::load L26 hex-encodes the sha256 by hand (L27-L30), verifies the digest and requires exactly 42 x-method entries (L49); validate L64 compiles one $defs entry with jsonschema.",
    },
    {
      file: "crates/collaboration-protocol/src/address_book_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types Existence L9, ArchiveState L16, StatusOrdering L23, LifecycleDisposition L30, AddressEntry L38 (lastStatus is the NativeThreadStatus projection), CoverageState L50, CoverageView L58, AddressPage L67",
    },
    {
      file: "crates/collaboration-protocol/src/approval_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "ApprovalDecision L18, ApprovalState L26, ApprovalOptionScope L38, ApprovalOfferedOption L49, ApprovalArgument L58, ApprovalPresentation L65, ApprovalOptionEffect L88, ApprovalOptionViewScope L96, ApprovalOptionView L104, ApprovalDetailedRecord L115 (requester is the board SessionRef), untagged ApprovalListResponse L137, ApprovalRequestRecord L144 (requester/approver are the protocol SessionRef; operation is untyped serde_json::Value L158), ApprovalDecideParams L170 (actor through interaction_actor L180).",
    },
    {
      file: "crates/collaboration-protocol/src/attempt_history_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types AttemptInspection L11, SummaryInspectionState L22, SummaryInspection L33, EarlierAttempts L55, AttemptHistoryCoverage L60, AttemptHistoryPage L68, DeliveryAttemptsRequest L76, RunSummariesRequest L84",
    },
    {
      file: "crates/collaboration-protocol/src/automation_configuration_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types AutomationConfiguration L7, AutomationConfigureRequest L21, AutomationStatus L36, ConfigurationFileState L45, ConfigurationFailure L52, ConfigurationFailureKind L61, ConfigurationNextAction L69",
    },
    {
      file: "crates/collaboration-protocol/src/automation_event_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types AutomationEventSubjectKind L11, AutomationEventSubject L21, AutomationEventDetails L32, AutomationEvent L61, AutomationEventsRequest L72, AutomationEventsPage L79",
    },
    {
      file: "crates/collaboration-protocol/src/automation_inspection_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types RunListRequest L11, DeliveryListRequest L36, RevisionListRequest L45, RevisionRecord L53, AutomationInspectionFailure L88 and its kind/stage/next-action enums L63-L85; an inline #[cfg(test)] mod tests sits at L18-L33.",
    },
    {
      file: "crates/collaboration-protocol/src/automation_page_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types PageLimit L6 (hand JsonSchema L25), InvalidPageLimit L9, AutomationPageRequest L35, AutomationPage L42",
    },
    {
      file: "crates/collaboration-protocol/src/automation_timing_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types PositiveSeconds L8 (hand JsonSchema L13), DEFAULT_EXECUTION_TIMEOUT L10, DEFAULT_SUMMARY_TIMEOUT L11, InvalidSeconds L24, TimingRequest L42, ExpiryRequest L59",
    },
    {
      file: "crates/collaboration-protocol/src/backend_generation.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types GenerationNumber L7 (JSON-safe integer 1..2^53-1), CodexGeneration L27",
    },
    {
      file: "crates/collaboration-protocol/src/cli_output_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "acp", "codex"] },
      note: "CLI output records: FiniteCommandRecord L17 (the result/error envelope), NativeObservationRecord L33 whose NativeMessage carries the Codex app-server message as an untyped JSON map (L41), ConversationRecord L62 with ACP update/result as untyped Value; acp_update_schema L124 and acp_result_schema L127 emit urn:agent-communication refs; bind_acp_schemas L133 and rewrite_references L153 rewrite them against the vendored ACP schema.",
    },
    {
      file: "crates/collaboration-protocol/src/conversation_create_outcome.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types ConversationCreateOutcome L16",
    },
    {
      file: "crates/collaboration-protocol/src/delivery_inspection_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types NativeEffectEvidence L11 (native turn and submission ids as strings), PreparationEffect L31, SubmissionEffect L40, CessationEvidence L49, DeliveryEvidence L61, DeliverySource L95, DeliveryDisposition L103, DeliveryInspection L112, DeliveryShowRequest L125, WakeMutationResult L130",
    },
    {
      file: "crates/collaboration-protocol/src/delivery_rejection.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types DeliveryRejectionReason L7 (17 reasons; collaboration-contracts native_delivery_result.rs:40 spells them again), DeliveryNextAction L29, DeliveryPeerClaim L41, DeliveryRejection L49",
    },
    {
      file: "crates/collaboration-protocol/src/delivery_route_evidence.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "ProviderAcpRouteEvidence alias L11, DeliveryRouteEvidence L21 (codexAppServer / providerAcp / claudeCodePeer)",
    },
    {
      file: "crates/collaboration-protocol/src/endpoint_description.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "acp", "codex"] },
      note: "EndpointDescription L131 with a hand Deserialize L139 through a mirror EndpointDocument L143 that checks channel count against availability, ChannelDescription L99, NativeCarrier L87, AcpCarrier L92, EndpointAvailability L72, SchemaDigest L29 (digest of the schema an endpoint speaks), ObservationTimestamp L54, NonEmptyText L11.",
    },
    {
      file: "crates/collaboration-protocol/src/endpoint_identity.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "SessionRef L107, EndpointRef L94, EndpointId L44, SessionId L69, UuidIdentity L12: the protocol-side identity; message-board board_identity.rs:211 has a second SessionRef with the same JSON.",
    },
    {
      file: "crates/collaboration-protocol/src/endpoint_inventory.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types EndpointInventory L6",
    },
    {
      file: "crates/collaboration-protocol/src/fleet_host_status.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types StuckMailSummary L13, KnownStuckMail L30",
    },
    {
      file: "crates/collaboration-protocol/src/identity_schemas.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "validated_schema! macro L10: hand-written JsonSchema for UuidIdentity, EndpointId, SessionId, NonEmptyText, SchemaDigest, ObservationTimestamp, GenerationNumber (L22-L49), patterns imported from message_board.",
    },
    {
      file: "crates/collaboration-protocol/src/installation_peer_identity.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "InstallationIdentity L9 (the MCP installation_show output), CentralLinkObservation L25 whose Available variant nests the central board_sync_protocol::FleetLinkRecord (L30-L32), so that central fleet-link shape is part of the MCP output schema; InstallationPeerId L56 (UUIDv7 RecordId, hand JsonSchema L81), InstallationPeerIdentityError L60.",
    },
    {
      file: "crates/collaboration-protocol/src/instruction_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types InstructionCreateParams L8, InstructionUpdateParams L14, InstructionShowParams L22, InstructionSnapshot L27, InstructionFailureKind L37, InstructionStage L48, InstructionNextAction L56, LocalMutationState L64, LocalMutationEvidence L71, InstructionFailure L76",
    },
    {
      file: "crates/collaboration-protocol/src/interaction_actor.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "deserialize_interaction_actor L7: untagged ActorWire L13 (typed board Identity or legacy protocol SessionRef); the legacy SessionRef is converted field by field to the board SessionRef (L20-L33).",
    },
    {
      file: "crates/collaboration-protocol/src/journal_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types JournalPosition L6, JournalBounds L13, LifecycleRecord L22, JournalPage L33, JournalStatus L43, JournalReadParams L49, AddressListParams L59",
    },
    {
      file: "crates/collaboration-protocol/src/lib.rs",
      disposition: { kind: "none" },
      note: "Module list and re-exports; it also re-exports the central board_sync_protocol::{FleetLinkRecord, FleetLinkState} (L20) and collaboration_mail_protocol::{MailDigest, MailItemId} (L339) through this facade. The test module push_line_tests is declared #[cfg(test)] at L119-L121.",
    },
    {
      file: "crates/collaboration-protocol/src/lifecycle_observation.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "codex", "mcp"] },
      note: "ThreadAddress L6, ObservationScope L12, ObservationSource L19, LifecycleSubject L27, BackendStatus L33, TerminalStatus L47 (the terminal Codex turn statuses), NativeActiveFlag L54 and NativeThreadStatus L61 (a pinned projection of Codex ThreadStatus, tag type), LifecycleChange L72, LifecycleObservation L95 with validate L103.",
    },
    {
      file: "crates/collaboration-protocol/src/mail_boundary_conversions.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "TryFrom<SessionRef> for message_board::SessionRef L9 and the reverse L17 convert through a serde_json to_value/from_value round trip; MessageDelivery to MailDeliveryMode L26 and back L35; CodexGeneration to GenerationGuard L44 and back L54; MailItemId to PushId L66 and back L72 through their strings.",
    },
    {
      file: "crates/collaboration-protocol/src/message_content.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "MessageText L9 (hand JsonSchema L63), MessageContent L75, MessageDelivery L92, MessageInputKind L101, MessageRepresentation L108, RenderedMessage L114 (no serde), AcceptedResumeEffect L242; session_identity L121 formats the emoji header identity. Inline #[cfg(test)] mod tests at L200-L238.",
    },
    {
      file: "crates/collaboration-protocol/src/message_frame_limit.rs",
      disposition: { kind: "none" },
      note: "MAX_CONTROL_FRAME_BYTES constant only (L6).",
    },
    {
      file: "crates/collaboration-protocol/src/message_mail_operations.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "MessageShowResult L10 and MessageHistoryEntry L26 (untagged local/remote), RemoteMessageStatus L17, MessageHistoryResult L32, MailReadContext L43, MessageReadCaller L62, MessageShowRequest L69, RemoteDeliveryStatus L91 / RemoteDeliveryRefusal L114 / RemoteProcessingFailure L144 / RemoteSizeField L156: public copies of the mail SDK's custody status (comment L82-L83).",
    },
    {
      file: "crates/collaboration-protocol/src/message_send_result.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "MessageSendResult L11 (untagged, schema anyOf rewritten to oneOf by disjoint_message_results L27), QueuedMessageSend L21; tests declared #[cfg(test)] at L35-L37.",
    },
    {
      file: "crates/collaboration-protocol/src/native_claude_inventory.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "Claude background-job inventory DTOs: NativeClaudeJobHandle L8 and NativeClaudeStateName L42 (hand JsonSchema L31, L57), NativeClaudeBackgroundState L68 with from_vendor_state L77 mapping Claude Code's working/blocked/done/failed/stopped strings, inventory part/reason enums L105-L158.",
    },
    {
      file: "crates/collaboration-protocol/src/native_control_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "codex", "mcp"] },
      note: "Native control DTOs for MCP tools over Codex: NativeInspectParams L10, NativeInspectResult L15 whose thread is an untyped serde_json::Value L26 bound only to the urn:codex-native:Thread ref, NativeRenameParams L30, NativeInterruptParams L43, NativeSendParams L64, NativeSendAcceptance L75 (turnId, submissionId), NativeSendReceipt L105.",
    },
    {
      file: "crates/collaboration-protocol/src/native_delivery_failure.rs",
      disposition: { kind: "defines", owners: ["codex", "router-vocabulary"] },
      note: "NativeDeliveryFailure L8; NativeDeliveryOperation L18 renames variants to Codex method strings (thread/read L21, thread/resume L23, thread/turns/list L25, turn/start L27, turn/steer L29, thread/queue/add L31): a method-name table; NativeDeliveryStage L37, NativeDeliveryFailureCause L47 with human_phrase L63.",
    },
    {
      file: "crates/collaboration-protocol/src/native_schema_references.rs",
      disposition: { kind: "defines", owners: ["codex"] },
      note: "thread_schema L3 and status_schema L6 emit urn:codex-native:Thread and urn:codex-native:ThreadStatus placeholder refs.",
    },
    {
      file: "crates/collaboration-protocol/src/native_session_catalog.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "codex", "mcp"] },
      note: "NativeSessionListParams L42, NativeSessionObservation L60 (Runtime.status is the Rust NativeThreadStatus projection but its schema is the urn:codex-native:ThreadStatus ref, L65-L66), NativeSessionSummary L72, NativeSessionListResult L88: sessions_list tool shapes carrying Codex thread facts.",
    },
    {
      file: "crates/collaboration-protocol/src/operation_failure_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "OperationFailureKind L9, OperationEffect L19, AdapterOperationFailure L26 (stage as String, provider code i64 and untyped data Value L32-L33)",
    },
    {
      file: "crates/collaboration-protocol/src/operation_inspection_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types OperationLocalMutation L11, OperationEffects L22, OperationFailure L36 (kind, stage, nextAction as String), OperationState L49, OperationSnapshot L69, OperationShowRequest L90",
    },
    {
      file: "crates/collaboration-protocol/src/operation_outcome_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "operation_methods! macro L9 builds AutomationOperationMethod L16 and six per-kind method enums L22-L27 from Router method-name literals (instruction/create ... automation/configure); OperationSuccess L31.",
    },
    {
      file: "crates/collaboration-protocol/src/permission_diagnostic.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types PermissionDiagnostic L6, PermissionDiagnosticKind L15, PermissionDiagnosticStage L21, PermissionDiagnosticNextAction L30",
    },
    {
      file: "crates/collaboration-protocol/src/protocol_type_schemas.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "protocol_type_schemas L7 exports 21 DTOs (L9-L29) as draft 2020-12 serialize-mode JSON Schema with schemars, then binds ACP refs into ConversationRecord (L30-L32).",
    },
    {
      file: "crates/collaboration-protocol/src/provider_conversation_contract.rs",
      disposition: {
        kind: "defines",
        owners: ["router-vocabulary", "acp", "router-extension", "mcp"],
      },
      note: "Provider conversation DTOs: ProviderKind L14, ProviderCapabilityName L76, ProviderBindingId L36 / ProviderCapabilities L114 / ProviderWorkingDirectory L202 with hand JsonSchema (L56, L147, L227), ProviderRequestedPolicy L245 with hand From to and from fleet_identity_protocol::RemoteLaunchPolicy (L249-L268), EffectiveProviderSettings L299, CodexPermissionModeReport L343 (carried on the create result's _meta.codexRouter.permissionMode; effective_settings L359), prompt and operation shapes L539-L632, ProviderPromptStopReason L636 (a Router copy of the session-event-model StopReason), settlements L652, ConversationOperationFailure L751.",
    },
    {
      file: "crates/collaboration-protocol/src/provider_identity.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "ProviderIdentity L12 (untagged: bare SessionRef or {humanId}); to_board_identity L35 and from_board_identity L61 convert SessionRef field by field to and from the board identity.",
    },
    {
      file: "crates/collaboration-protocol/src/provider_session_inspect.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "acp", "mcp"] },
      note: "ProviderSessionInspectResult L23 (capabilities is session-event-model CapabilityReport), ProviderSettingsCatalogView L42, ProviderSettingChoiceView L50, ProviderConfigOptionView L57 and ProviderConfigValueView L68: Router views of an ACP provider session's modes and config options; ProviderInspectFailure L83.",
    },
    {
      file: "crates/collaboration-protocol/src/provider_session_list.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "ProviderSessionSummary L44 (untagged) with Claude Code origins (HostedProviderOrigin L88, ClaudeCodeInteractiveOrigin L94, ClaudeCodeBackgroundOrigin L100) and Claude Code registry facts (kind, entrypoint, status converted from registry milliseconds L69-L83; ClaudeCodeInteractiveStatus L123; NativeClaudePeerObservation L134).",
    },
    {
      file: "crates/collaboration-protocol/src/provider_settings_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "acp", "mcp"] },
      note: "ProviderSettingsSetRequest L9, ProviderSettingsAcceptRequest L18, ProviderSettingsResult L25, ProviderSettingsFailureKind L32, ProviderSettingsFailure L45.",
    },
    {
      file: "crates/collaboration-protocol/src/push_line.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "The push line text format a recipient agent reads: MachineLabel L18 (hand JsonSchema L60), MachineId L72, PushId L104, RouterLink L135 with RouterLink::parse L149 and Display L172 (router://<machine>/push/<id>), PushKind L185, PushOrigin L198, PushHeaderFacts L211, MAX_PUSH_LINE_BYTES L9; includes push_line_render.rs (L305) and push_line_parse.rs (L309) as path modules.",
    },
    {
      file: "crates/collaboration-protocol/src/push_line_parse.rs",
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "parse_push_line L5 recognises Router deliveries in agent-visible text (string splitting on ' · ', ' @', ' 📖 Load full message → '); tests declared #[cfg(test)] at L44-L46.",
    },
    {
      file: "crates/collaboration-protocol/src/push_line_render.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "render_push_line L342 renders a PushLineInput to the push line text (DisplayLimits::render L56 builds the emoji headers, preview and link; largest_fitting_prefix L310 clips to the byte budget); parse_push_line_header L367 classifies a header back to PushKind by its emoji prefix.",
    },
    {
      file: "crates/collaboration-protocol/src/push_record.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "PushRecord L237 and PushRecordDraft L63 (originRouterRef carried as Option<String> L67, L241, parsed with RouterOriginRef::parse_canonical L123), PushActivitySnapshot L26, PushDeliveryState L189, CancelledAfterDelivery L201, list/show/history params and results L263-L331: the stored and shown mail record.",
    },
    {
      file: "crates/collaboration-protocol/src/question_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "QuestionListParams L9, QuestionFieldView L15, QuestionState L59, QuestionRecord L68 (requester is the board SessionRef), QuestionListResult L79, QuestionAnswerParams L85, QuestionAnswerResult L94; re-exports session_event_model ChoiceOption, QuestionAnswerValue, QuestionResponse (L5).",
    },
    {
      file: "crates/collaboration-protocol/src/router_executable_relation.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "RouterExecutableRelation L8 (serde, snake_case tag): how the running Router build relates to its launch path; router_build_warning L27; inline #[cfg(test)] mod tests at L41.",
    },
    {
      file: "crates/collaboration-protocol/src/router_origin_ref.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "RouterOriginRef L11 and its mirror RouterOriginRefWire L131 (to_wire L189, from_wire L234); canonical_string L174 and parse_canonical L178 carry the reference as a JSON string inside a string field and reject any non-canonical encoding; Serialize L276 and Deserialize L286 use that string. Inline #[cfg(test)] mod tests at L306.",
    },
    {
      file: "crates/collaboration-protocol/src/run_evidence_validation.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "RunSnapshot's only Deserialize (RunSnapshot derives Serialize only, run_execution_contract.rs:154): RunSnapshotFields L12 is a field-for-field mirror decoded at L22 and then checked by validate_evidence L43 for cross-field evidence.",
    },
    {
      file: "crates/collaboration-protocol/src/run_execution_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types ContinuityInput L17, FrozenExecutionConfiguration L37, CapturedRunInputs L48, WorkerOutcome L57, RunState L81, ExecutionTiming L130, RunExecutionEvidence L137, RetainedSummary L147, RunSnapshot L156 (Serialize only), RunShowRequest L167, RunRecoveryRequest L172",
    },
    {
      file: "crates/collaboration-protocol/src/run_execution_reference.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types NativeExecution L8 (turn_id renamed nativeTurnId L10), RunExecution L24",
    },
    {
      file: "crates/collaboration-protocol/src/run_failure_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types RunFailureKind L7, RunFailureStage L18, RunNextAction L26, RunFailure L34",
    },
    {
      file: "crates/collaboration-protocol/src/schedule_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types ExecutionDestination L11, ScheduleDefinition L23 (hand JSON Schema if/then L19-L22), ImportedContinuity L42, ScheduleSnapshot L53, ScheduleCreateRequest L70, ScheduleUpdateRequest L76, ScheduleShowRequest L84, ScheduleEnableRequest L89",
    },
    {
      file: "crates/collaboration-protocol/src/schedule_failure.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types ScheduleFailureKind L8, ScheduleFailureStage L23, ScheduleNextAction L32, ScheduleFailure L42, ScheduleFailureDetails L68, ScheduleEffects L101",
    },
    {
      file: "crates/collaboration-protocol/src/schedule_package_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types ScheduleImportRequest L8, ScheduleExportResult L16",
    },
    {
      file: "crates/collaboration-protocol/src/schedule_preparation_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types DestinationPreparation L12, SchedulePrepareRequest L29",
    },
    {
      file: "crates/collaboration-protocol/src/service_manifest.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (local service discovery file)",
      },
      note: "ServiceManifest L59 (SERVICE_MANIFEST_VERSION L10, read_version L78): published endpoint and MCP selectors (ApiSelector L38, McpSelector L52); json! only in the inline #[cfg(test)] mod tests at L86.",
    },
    {
      file: "crates/collaboration-protocol/src/session_delivery_outcome.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types DeliveryIdentityError L11, DeliveryCorrelationId L15 (UUIDv7 generate L34, hand JsonSchema L44), DeliveryOutcome L59, SessionReachability L72",
    },
    {
      file: "crates/collaboration-protocol/src/session_delivery_receipt.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types DeliveryClientReceipt L15, DeliveryReceipt L23; From<NativeSendReceipt> L29 maps NativeSendAcceptance to DeliveryOutcome by hand.",
    },
    {
      file: "crates/collaboration-protocol/src/session_message_reply.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types SessionMessageReplyParams L8, SessionMessageReplyResult L18",
    },
    {
      file: "crates/collaboration-protocol/src/session_message_send.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types SessionMessageSendParams L8",
    },
    {
      file: "crates/collaboration-protocol/src/session_observation_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "codex", "mcp"] },
      note: "BoundedObservationResult L21 (events: Vec<Value> L25, native Codex messages or provider hub events, untyped), BoundedObservationRequest L35, OBSERVATION_EVENT_NOTIFICATION L67 = notifications/codexRouter/observationEvent, the custom MCP notification name; ObservationEventNotification L72 carries the event as untyped JSON and provider_event L92 reads its sequence field by hand. Inline #[cfg(test)] mod tests at L123.",
    },
    {
      file: "crates/collaboration-protocol/src/thread_subscription_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types ThreadSubscribeRequest L12, ThreadUnsubscribeRequest L23, ThreadSubscriptionsRequest L33, ThreadSubscriptionsResult L39, ThreadSubscriptionState L46, ThreadSubscriptionPresence L59, ThreadSubscriptionView L67",
    },
    {
      file: "crates/collaboration-protocol/src/thread_subscription_wait.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types MAX_SUBSCRIPTION_WAIT_SECONDS L12, ThreadSubscriptionWaitFilter L21, ThreadSubscriptionWaitRequest L33 decoded through RawThreadSubscriptionWaitRequest L45 (try_from L63), InvalidSubscriptionWait L57, SubscriptionWaitBatch L106, ThreadSubscriptionWaitResult L127, ThreadSubscriptionWaitOutcome L152",
    },
    {
      file: "crates/collaboration-protocol/src/wake_authority.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types WakeCreator L9, WakeRefusal L18",
    },
    {
      file: "crates/collaboration-protocol/src/wakeup_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types SavedMessage L12 (DurableMessage::delivery_mode L32 spells auto/steer/queue by hand), WakeSendRequest L42, WakeShowRequest L52, WakeMutationRequest L57, WakeState L63, WakeDefinition L72, FireReceipt L90, WakeSnapshot L99",
    },
    {
      file: "crates/collaboration-protocol/src/wakeup_failure.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types WakeFailureReason L8, WakeFailureStage L26, WakeNextAction L34, WakeFailure L43 with a hand Deserialize L56 (Envelope L62, reason fields split from a JSON map and kind literals L93-L102, then from_value L107 and L114)",
    },
    {
      file: "crates/collaboration-protocol/src/wakeup_subscription_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "types WakeSubscription L8, WakeChange L15, WakeChanged L24, WaitUnavailable L32, WaitUnavailableKind L44, WakeNotFound L76, WakeWaitRequest L106, WakeWaitResult L120, WakeWaitOutcome L129 (From<WakeChange> L141)",
    },
  ],
  sendSites: [],
};
