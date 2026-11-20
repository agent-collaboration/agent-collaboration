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
    line: 9,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "NativeEffectEvidence {permissionDifferences, target: SessionRef, generation: CodexGeneration, clientUserMessageId, nativeTurnId, nativeSubmissionId, allocation, resume, submission, cessation} with its own PreparationEffect L29, SubmissionEffect L38 (no routerQueued) and CessationEvidence L47: a separately written copy of agent-automation native_effects.rs:43 (and :17, :26, :36)",
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
