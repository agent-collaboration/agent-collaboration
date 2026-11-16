import type { LedgerSection } from "../schema-audit-types.ts";
// Left out by name only: 0 colocated unit-test files (*_test.rs, *_tests.rs).
// The types here are Router vocabulary: collaboration-protocol re-exports them (lib.rs:233-234,
// 310, 322) and nests them in the automation tool results the MCP edge advertises.

export const section: LedgerSection = {
  root: "crates/agent-automation",
  files: [
    {
      file: "crates/agent-automation/src/accepted_delivery_effect.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "AcceptedDeliveryEffect L6 (camelCase serde enum).",
    },
    {
      file: "crates/agent-automation/src/automation_identity.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "automation_identity! L13 defines 13 UUIDv7 string newtypes (ScheduleId ... SubscriptionId L53-L67; OperationId among them, re-exported as collaboration_protocol::OperationId) with serde try_from/into String L16, a hand-written JsonSchema with a UUIDv7 pattern L18-L23 and generate() as Uuid::now_v7 L47; AutomationIdentityError L9.",
    },
    {
      file: "crates/agent-automation/src/delivery_state.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "DeliveryStatus L5 (camelCase serde enum) and may_have_native_effect L16.",
    },
    {
      file: "crates/agent-automation/src/instruction_document.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "InstructionText L11 (try_from String) with a hand-written JsonSchema L42 (x-maxUtf8Bytes); InstructionDocument L34; InstructionTextError L7.",
    },
    {
      file: "crates/agent-automation/src/lib.rs",
      disposition: { kind: "none" },
      note: "Module list and re-exports.",
    },
    {
      file: "crates/agent-automation/src/native_effects.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "codex"] },
      note: "How far a Codex turn start or steer got: PermissionDifference L8 (JsonSchema), PreparationEffect L17, SubmissionEffect L26, CessationEvidence L36, NativeEffectEvidence L43 (client_user_message_id, native_turn_id, native_submission_id as plain Strings), AttemptOutcome L58, DeliveryAttempt L66 with a hand Deserialize through DeliveryAttemptFields L80/L90 that rejects inconsistent attempt states.",
    },
    {
      file: "crates/agent-automation/src/portable_schedule_encoding.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "PortableRecord L12: closed JSONL records (packageHeader agentSchedule v1, instructionDocument, scheduleDefinition, continuitySummary, packageEnd with record count and SHA-256). encode_schedule_package L37 / decode_schedule_package L83 produce and read the text that MCP schedule_export / schedule_import carry as the packageUtf8 string (collaboration-protocol schedule_package_contract.rs:10, :17).",
    },
    {
      file: "crates/agent-automation/src/portable_schedule_package.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "PortableInstruction L5, PortableContinuity L11, PortableSchedulePackage L17 (serde, no rename: snake_case fields), PackageError L25.",
    },
    {
      file: "crates/agent-automation/src/route_effect_evidence.rs",
      disposition: {
        kind: "defines",
        owners: ["router-vocabulary", "acp", "codex"],
      },
      note: "RouteEffectEvidence L9 (tag kind: codexAppServer, providerAcp, claudeCodePeer) with a hand Deserialize L306 through TaggedRouteEffectEvidence L293 or an untagged legacy bare NativeEffectEvidence L299-L304; ProviderAcpEffectEvidence L104 (hand Deserialize L136 via ProviderAcpEffectFields L127), ProviderSettlementEffect L166, ProviderBindingReference L174, PeerWriteEffect L207, PeerProcessId L216, PeerSessionReference L220 (hand JsonSchema L222), ClaudeCodePeerEffectEvidence L285.",
    },
    {
      file: "crates/agent-automation/src/run_execution.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "RunExecutionEvidence L5 (route accepts the legacy field name native, alias L6) with a hand Deserialize L22 via RunExecutionFields L15; ExecutionTiming L48, WorkerOutcome L67, RunRecord L75.",
    },
    {
      file: "crates/agent-automation/src/run_lifecycle.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "RunPhase L6 (camelCase serde enum).",
    },
    {
      file: "crates/agent-automation/src/schedule_definition.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "ExecutionDestination L7, ExecutionMode L14 (not serde), ScheduleDefinition L38, ContinuityInput L50 (sourceReference accepts legacy sourceTurnId L63), FrozenExecutionConfiguration L81, CapturedRunInputs L90, SummarySource L101, ScheduleRecord L121.",
    },
    {
      file: "crates/agent-automation/src/summary_attempt.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "SummaryPhase L5, SummaryAttempt L16 (source_reference accepts legacy sourceTurnId L20).",
    },
    {
      file: "crates/agent-automation/src/summary_source_reference.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "codex"] },
      note: "SummarySourceReference L12 (nativeTurn {turnId}, JsonSchema); deserialize_stored_source_reference L25 also accepts a bare legacy turn id string (StoredReference L33).",
    },
    {
      file: "crates/agent-automation/src/timing_calculation.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "TimingRule L20 (tag kind: at, after, interval, cron) and its next_due calculation L39; TimingError L7.",
    },
    {
      file: "crates/agent-automation/src/wakeup_definition.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "ExpiryRule L8, WakeState L32, FirstFire L41, WakeDefinition L49, WakeRecord L61; DurableMessage L71 lets adapters expose a message's target, content and generation, and its delivery mode as a bare &'static str (L78).",
    },
  ],
  sendSites: [],
};
