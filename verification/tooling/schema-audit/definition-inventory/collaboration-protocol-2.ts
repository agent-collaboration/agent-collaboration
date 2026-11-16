import type { InventoryRow } from "../schema-audit-types.ts";

const P = "crates/collaboration-protocol/src/";

/**
 * The public MCP data types collaboration-protocol defines (R51 omission sweep, OB5 group 6).
 * Each is the one typed definition the MCP catalogue advertises, so it is a boundary encoding;
 * a separately written copy of the same contract is a duplicate row. A stored automation record
 * that the service projects into one of these public types is a different contract, not a copy.
 */
export const rows: InventoryRow[] = [
  {
    file: `${P}delivery_inspection_contract.rs`,
    line: 11,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "NativeEffectEvidence {permissionDifferences, target: SessionRef, generation: CodexGeneration, clientUserMessageId, nativeTurnId, nativeSubmissionId, allocation, resume, submission, cessation} with its own PreparationEffect L31, SubmissionEffect L40 (no routerQueued) and CessationEvidence L49: a separately written copy of agent-automation native_effects.rs:43 (and :17, :26, :36). collaboration-runtime codex_app_server_scheduled_runs.rs:43-51 JSON round-trips this public copy into the automation one",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-runtime" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: "native-effect-evidence",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${P}delivery_inspection_contract.rs`,
    line: 61,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP delivery_show / delivery_list / delivery_reconcile result: DeliveryEvidence L61, DeliverySource L95, DeliveryDisposition L103, DeliveryInspection L112, DeliveryShowRequest L125, and WakeMutationResult L130",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}schedule_contract.rs`,
    line: 23,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP ScheduleDefinition {instructionId, timing: TimingRequest, enabled, destination, executionTimeoutSeconds: PositiveSeconds, model, effort} (with the freshEachRun schema rule L19), carried by ScheduleCreateRequest L70, ScheduleUpdateRequest L76 and ScheduleSnapshot L53 (ImportedContinuity L42, ScheduleShowRequest L84, ScheduleEnableRequest L89). The canonical MCP definition; agent-automation schedule_definition.rs:38 is a separately written copy that collaboration-service schedule_operations.rs:48-50,100-102 converts into by serde_json",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
      { crate: "agent-collaboration" },
    ],
    duplicateGroup: "schedule-definition-copies",
    disposition: "boundary-encoding",
  },
  {
    file: `${P}schedule_contract.rs`,
    line: 11,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP ExecutionDestination (tag kind: unprepared, freshEachRunUnprepared, ownedThread {target: SessionRef, cwd}, freshEachRun {endpoint: EndpointRef, cwd}) nested in ScheduleDefinition; agent-automation schedule_definition.rs:7 is a separately written generic copy",
    usedBy: [{ part: "N11" }, { crate: "collaboration-service" }],
    duplicateGroup: "schedule-definition-copies",
    disposition: "boundary-encoding",
  },
  {
    file: `${P}automation_timing_contract.rs`,
    line: 42,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP TimingRequest (tag kind: at {at: ObservationTimestamp}, after / interval {seconds: PositiveSeconds}, cron {expression, timezone}) and ExpiryRequest L59, nested in ScheduleDefinition and the wake requests; agent-automation timing_calculation.rs:20 TimingRule and collaboration-mail-protocol remote_wake.rs:104 RemoteWakeTiming are separately written copies",
    usedBy: [{ part: "N11" }, { crate: "collaboration-service" }, { crate: "agent-collaboration" }],
    duplicateGroup: "wake-record-copies",
    disposition: "boundary-encoding",
  },
  {
    file: `${P}schedule_package_contract.rs`,
    line: 8,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP schedule_import ScheduleImportRequest {operationId, packageUtf8, overwrite} and schedule_export ScheduleExportResult {packageUtf8} L16: the string carries the portable JSONL export (agent-automation portable_schedule_encoding.rs:12)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
      { crate: "agent-collaboration" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}schedule_preparation_contract.rs`,
    line: 12,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP schedule_prepare SchedulePrepareRequest L29 {operationId, scheduleId, destination} with DestinationPreparation fresh {endpoint, cwd} / fork {source, throughTurnId, cwd} / existing {target, cwd}",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}schedule_failure.rs`,
    line: 42,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP schedule tool failure data: ScheduleFailure L42 with ScheduleFailureKind L8, ScheduleFailureStage L23, ScheduleNextAction L32, ScheduleFailureDetails L68 and ScheduleEffects L101",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}session_message_send.rs`,
    line: 8,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP message_send SessionMessageSendParams {itemId?, target: SessionRef, message: MessageContent, mode: MessageDelivery, generationGuard: CodexGeneration?}, also exported by protocol_type_schemas.rs:19",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}session_message_reply.rs`,
    line: 8,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP message_reply SessionMessageReplyParams {caller, reference, text} and SessionMessageReplyResult L18 {target, targetIdentity, pushId, link, deliveryState, receipt}, also exported by protocol_type_schemas.rs:20-21",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}message_send_result.rs`,
    line: 11,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP message_send / message_reply result MessageSendResult<TLocal> (local or queued), MessageReplyResult alias L17, QueuedMessageSend L21",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}message_mail_operations.rs`,
    line: 10,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP router_show (tool registration message_tools.rs:59-65; message_show is the internal application method) and message_history shapes: MessageShowResult L10, RemoteMessageStatus L17, MessageHistoryEntry L26, MessageHistoryResult L32, MailReadContext L43, HumanMessageReader L56, MessageReadCaller L62, MessageShowRequest L69, RemoteDeliveryStatus L91, RemoteDeliveryRefusal L114, RemoteProcessingFailure L144, RemoteSizeField L156",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}delivery_rejection.rs`,
    line: 49,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "DeliveryRejection L49 {reason: DeliveryRejectionReason L7, nextAction: DeliveryNextAction L29, clientCode?, detail?, claims?: [DeliveryPeerClaim L41], nativeFailure?: NativeDeliveryFailure}, nested in the DeliveryOutcome of MCP message results",
    usedBy: [{ part: "N11" }, { crate: "collaboration-service" }, { crate: "codex-router-host" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}delivery_route_evidence.rs`,
    line: 21,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "public DeliveryRouteEvidence (tag kind: codexAppServer(NativeEffectEvidence), providerAcp {bindingId, generation, target, operationId, submission}, claudeCodePeer {sessionId, write}) in run and delivery inspection results; ProviderAcpRouteEvidence L11 is an alias of the agent-automation type, not a copy. The stored agent-automation RouteEffectEvidence is a different (stored) contract the service projects into this one",
    usedBy: [{ part: "N11" }, { crate: "collaboration-service" }, { crate: "agent-collaboration" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}run_execution_contract.rs`,
    line: 156,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP run_show / run_list RunSnapshot L156 with RunState L81, ContinuityInput L17, FrozenExecutionConfiguration L37, CapturedRunInputs L48, WorkerOutcome L57, ExecutionTiming L130, RunExecutionEvidence L137, RetainedSummary L147, RunShowRequest L167, RunRecoveryRequest L172. Public run state projected by collaboration-service run_projection.rs from the stored agent-automation RunRecord (a different, stored contract with millisecond times)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
      { crate: "agent-collaboration" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}run_execution_reference.rs`,
    line: 24,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RunExecution (tag kind: codexAppServer(NativeExecution L8 {target, nativeTurnId, startedAt, deadlineAt, effectiveTimeoutSeconds}), providerAcp, claudeCodePeer) nested in RunState",
    usedBy: [{ part: "N11" }, { crate: "collaboration-service" }, { crate: "codex-router-host" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}run_failure_contract.rs`,
    line: 34,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP run tool failure data RunFailure L34 with RunFailureKind L7, RunFailureStage L18, RunNextAction L26",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}attempt_history_contract.rs`,
    line: 68,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP delivery_attempts / run_summaries AttemptHistoryPage<TRecord> L68 with AttemptInspection L11, SummaryInspectionState L22, SummaryInspection L33, EarlierAttempts L55, AttemptHistoryCoverage L60, DeliveryAttemptsRequest L76, RunSummariesRequest L84",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}automation_configuration_contract.rs`,
    line: 7,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP automation_configure / automation_status: AutomationConfiguration L7, AutomationConfigureRequest L21, AutomationStatus L36, ConfigurationFileState L45, ConfigurationFailure L52, ConfigurationFailureKind L61, ConfigurationNextAction L69",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
      { crate: "codex-router-host" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}automation_event_contract.rs`,
    line: 61,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP automation_events AutomationEventsPage L79 of AutomationEvent L61 {subject: AutomationEventSubject L21 (AutomationEventSubjectKind L11), details: AutomationEventDetails L32}, AutomationEventsRequest L72",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}automation_inspection_contract.rs`,
    line: 11,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP run_list / delivery_list / revision_list requests RunListRequest L11, DeliveryListRequest L36, RevisionListRequest L45, RevisionRecord L53, and failure data AutomationInspectionFailure L88 (kind L63, stage L73, nextAction L80)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
      { crate: "agent-collaboration" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}instruction_contract.rs`,
    line: 8,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP instruction_create / update / show: InstructionCreateParams L8, InstructionUpdateParams L14, InstructionShowParams L22, InstructionSnapshot L27, and failure data InstructionFailure L76 (kind L37, stage L48, nextAction L56, LocalMutationState L64, LocalMutationEvidence L71)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
      { crate: "agent-collaboration" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}operation_inspection_contract.rs`,
    line: 69,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP operation_show OperationSnapshot L69 with OperationLocalMutation L11, OperationEffects L22, OperationFailure L36, OperationState L49, OperationShowRequest L90",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}provider_settings_contract.rs`,
    line: 9,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP conversation_settings_set / accept: ProviderSettingsSetRequest L9, ProviderSettingsAcceptRequest L18, ProviderSettingsResult L25, ProviderSettingsFailure L45 (kind L32)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
      { crate: "codex-router-host" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}question_contract.rs`,
    line: 15,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP question_list / question_answer: QuestionListParams L9, QuestionFieldView L15, public unit QuestionState L59, QuestionRecord L68, QuestionListResult L79, QuestionAnswerParams L85, QuestionAnswerResult L94. The stored QuestionHistoryState is a different contract projected into these (collaboration-service interaction_operations.rs:220-255)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}conversation_create_outcome.rs`,
    line: 16,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP conversation_create result ConversationCreateOutcome (tag kind: created {operationId, target, effectiveSettings?, permissionDifferences}, createdWithoutSettings {operationId, target, applied, failed, notApplied}, pending {operationId})",
    usedBy: [{ part: "N11" }, { crate: "collaboration-client" }, { crate: "agent-collaboration" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}thread_subscription_contract.rs`,
    line: 12,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP thread_subscribe / thread_unsubscribe / thread_subscriptions: ThreadSubscribeRequest L12, ThreadUnsubscribeRequest L23, ThreadSubscriptionsRequest L33, ThreadSubscriptionsResult L39, ThreadSubscriptionState L46, ThreadSubscriptionPresence L59, ThreadSubscriptionView L67",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}thread_subscription_wait.rs`,
    line: 33,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP thread_wait: ThreadSubscriptionWaitFilter L21, ThreadSubscriptionWaitRequest L33, InvalidSubscriptionWait L57, SubscriptionWaitBatch L106, ThreadSubscriptionWaitResult L127, SubscriptionRenewalEntry L134, SubscriptionRenewalOutcome L140, ThreadSubscriptionWaitOutcome L152",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
      { crate: "agent-collaboration" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}wake_authority.rs`,
    line: 9,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "WakeCreator (tag kind: session(SessionRef), owner, router, unrecorded) nested in wake requests and WakeDefinition (wakeup_contract.rs:44, :75), and WakeRefusal L18 nested in WakeFailure (wakeup_failure.rs:22)",
    usedBy: [{ part: "N11" }, { crate: "collaboration-service" }, { crate: "agent-collaboration" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}address_book_contract.rs`,
    line: 67,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP addresses_list AddressPage L67 of AddressEntry L38 with Existence L9, ArchiveState L16, StatusOrdering L23, LifecycleDisposition L30, CoverageState L50, CoverageView L58",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}journal_contract.rs`,
    line: 33,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP journal_read / journal_status / addresses_list: JournalPosition L6, JournalBounds L13, LifecycleRecord L22, JournalPage L33, JournalStatus L43, JournalReadParams L49, AddressListParams L59",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}endpoint_inventory.rs`,
    line: 6,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP endpoints_list EndpointInventory {serviceEpoch, sequence (0..2^53-1), endpoints (at most 64 EndpointDescription)}",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
      { crate: "codex-router-host" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${P}backend_generation.rs`,
    line: 27,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "CodexGeneration {serviceEpoch, generation: GenerationNumber L7 (try_from u64, 1..2^53-1)}: the generation guard nested in MCP message_send and in delivery evidence",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-service" },
      { crate: "collaboration-client" },
      { crate: "codex-router-host" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
];
