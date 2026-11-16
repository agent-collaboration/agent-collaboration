import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
  {
    file: "crates/agent-automation/src/automation_identity.rs",
    line: 18,
    owner: "router-vocabulary",
    kind: "schema-mechanism",
    defines:
      "automation_identity! hand-writes the JsonSchema of 13 UUIDv7 identity newtypes (string, length 36, UUIDv7 pattern) and their String try_from/into, including the OperationId every MCP operation tool takes",
    usedBy: [{ part: "N11" }, { crate: "collaboration-protocol" }],
    duplicateGroup: "schema-mechanism",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/agent-automation/src/instruction_document.rs",
    line: 42,
    owner: "router-vocabulary",
    kind: "schema-mechanism",
    defines:
      "hand-written JsonSchema for InstructionText (minLength 1, maxLength 1048576, NUL-free pattern, x-maxUtf8Bytes)",
    usedBy: [{ part: "N11" }, { crate: "collaboration-protocol" }],
    duplicateGroup: "schema-mechanism",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/agent-automation/src/native_effects.rs",
    line: 43,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "NativeEffectEvidence<TTarget, TGeneration>: Router record of a Codex turn start or steer, holding Codex identities client_user_message_id, native_turn_id and native_submission_id as plain Strings, with allocation, resume, submission and cessation effects. A second, separately written copy of the same contract is collaboration-protocol delivery_inspection_contract.rs:11 (SessionRef/CodexGeneration, no routerQueued SubmissionEffect); collaboration-runtime codex_app_server_scheduled_runs.rs:43-51 JSON round-trips the public copy into this one",
    usedBy: [
      { crate: "collaboration-protocol" },
      { crate: "automation-storage" },
      { crate: "collaboration-runtime" },
    ],
    duplicateGroup: "native-effect-evidence",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/agent-automation/src/portable_schedule_encoding.rs",
    line: 12,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "PortableRecord JSONL schedule package (packageHeader agentSchedule v1, instructionDocument, scheduleDefinition, continuitySummary, packageEnd with record count and SHA-256), carried inside MCP schedule_export / schedule_import as the packageUtf8 string. Intentional export format; no defect. Its nested ScheduleDefinition, ExecutionDestination and TimingRule have their own rows",
    usedBy: [{ part: "N11" }, { crate: "collaboration-service" }, { crate: "automation-storage" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/agent-automation/src/schedule_definition.rs",
    line: 38,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "ScheduleDefinition<TTarget, TEndpoint> {instructionId, timing: TimingRule, enabled, destination, executionTimeoutSeconds, model, effort} (camelCase, deny_unknown_fields): nested in the portable package scheduleDefinition record (portable_schedule_encoding.rs:22-25) and so in MCP packageUtf8; a separately written copy of the MCP collaboration-protocol ScheduleDefinition (schedule_contract.rs:23, TimingRequest/PositiveSeconds and a freshEachRun schema rule), into which collaboration-service schedule_operations.rs:48-50,100-102 converts by serde_json to_value then from_value",
    usedBy: [{ part: "N11" }, { crate: "collaboration-service" }, { crate: "automation-storage" }],
    duplicateGroup: "schedule-definition-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/agent-automation/src/schedule_definition.rs",
    line: 7,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "ExecutionDestination<TTarget, TEndpoint> (tag kind: unprepared, freshEachRunUnprepared, ownedThread {target, cwd}, freshEachRun {endpoint, cwd}): nested in ScheduleDefinition and the portable package; a separately written copy of collaboration-protocol ExecutionDestination (schedule_contract.rs:11, SessionRef/EndpointRef)",
    usedBy: [{ part: "N11" }, { crate: "collaboration-service" }, { crate: "automation-storage" }],
    duplicateGroup: "schedule-definition-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/agent-automation/src/timing_calculation.rs",
    line: 20,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "TimingRule (tag kind: at {at: DateTime<Utc>}, after {seconds: u32}, interval {seconds: u32}, cron {expression, timezone}): nested in ScheduleDefinition and the portable package; a separately written copy of collaboration-protocol TimingRequest (automation_timing_contract.rs:42, ObservationTimestamp/PositiveSeconds), converted to it by serde_json at collaboration-service wakeup_projection.rs:48",
    usedBy: [{ part: "N11" }, { crate: "collaboration-service" }, { crate: "automation-storage" }],
    duplicateGroup: "wake-record-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/agent-automation/src/route_effect_evidence.rs",
    line: 9,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RouteEffectEvidence: one kind-tagged enum over Codex app-server, provider ACP and Claude Code peer effects, generic over target and generation; its JsonSchema is not here (only PeerSessionReference has a hand schema, L222)",
    usedBy: [{ part: "N4" }, { crate: "collaboration-protocol" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/agent-automation/src/route_effect_evidence.rs",
    line: 299,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "hand Deserialize of RouteEffectEvidence (L306): tagged form, or an untagged legacy bare NativeEffectEvidence read as codexAppServer, then rejects routerQueued outside providerAcp",
    usedBy: [{ crate: "collaboration-protocol" }, { crate: "automation-storage" }],
    duplicateGroup: "legacy-stored-decoders",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/agent-automation/src/route_effect_evidence.rs",
    line: 220,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "PeerSessionReference: the Claude Code session id as its own string newtype (1 to 4096 bytes, hand JsonSchema L222), while claude-code-peer-messaging carries the same id as collaboration_protocol::SessionId",
    usedBy: [{ crate: "collaboration-protocol" }, { crate: "codex-router-host" }],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/agent-automation/src/summary_source_reference.rs",
    line: 25,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "deserialize_stored_source_reference: a stored bare turn id string (legacy) becomes SummarySourceReference::NativeTurn {turnId}; used by ContinuityInput, SummarySource and SummaryAttempt under the legacy alias sourceTurnId",
    usedBy: [{ crate: "collaboration-protocol" }, { crate: "automation-storage" }],
    duplicateGroup: "legacy-stored-decoders",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/agent-automation/src/run_execution.rs",
    line: 6,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RunExecutionEvidence route field accepts the legacy name native (alias L6, L16) and a hand Deserialize (L22) refuses timing or acceptance without a route",
    usedBy: [{ crate: "collaboration-protocol" }, { crate: "automation-storage" }],
    duplicateGroup: "legacy-stored-decoders",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/agent-automation/src/wakeup_definition.rs",
    line: 78,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "DurableMessage::delivery_mode returns a message's delivery mode as a bare &'static str instead of the delivery-mode type",
    usedBy: [{ crate: "collaboration-protocol" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
];
