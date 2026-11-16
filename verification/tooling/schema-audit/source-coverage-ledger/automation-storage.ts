import type { LedgerFile, LedgerSection } from "../schema-audit-types.ts";

const A = "crates/automation-storage/src/";
const vocabulary = ["router-vocabulary" as const];

function none(file: string, note: string): LedgerFile {
  return { file: `${A}${file}`, disposition: { kind: "none" }, note };
}
function converts(file: string, note: string): LedgerFile {
  return { file: `${A}${file}`, disposition: { kind: "converts", owners: vocabulary }, note };
}
function defines(file: string, note: string): LedgerFile {
  return { file: `${A}${file}`, disposition: { kind: "defines", owners: vocabulary }, note };
}
function reads(file: string, note: string): LedgerFile {
  return { file: `${A}${file}`, disposition: { kind: "reads", owners: vocabulary }, note };
}

export const section: LedgerSection = {
  root: "crates/automation-storage",
  files: [
    reads(
      "attempt_history_read.rs",
      "decode_attempt l234 reads attemptId and startedAtMs from stored attempt JSON by key; SQL json_extract of $.attempt.* l171-172, l219; StoredAttemptRecord l23 returns untyped Value bodies",
    ),
    none(
      "automation_collection_listing.rs",
      "AutomationListKey l14 and AutomationListPosition l19 page keys over SQL; run_timestamp l206 reads the UUIDv7 time of a RunId; no message shape",
    ),
    none(
      "automation_connection.rs",
      "StorageError l14 (thiserror, no serde) and the SQLite connection lifecycle",
    ),
    none("automation_event_bounds.rs", "retention cutoff arithmetic over millisecond stamps"),
    none(
      "automation_event_history.rs",
      "StoredAutomationEvent l15 returns event rows with subject_kind/event_kind strings and an untyped Value body; typed by collaboration-service automation_event_projection.rs; no field is parsed here",
    ),
    none(
      "automation_retention.rs",
      "retention DELETE and next-due SQL per AutomationRetentionClass l19",
    ),
    converts(
      "configuration_receipts.rs",
      "hand-built OperationEffects::Configuration json! l35, l80, l137 and event configurationChange l146; method literal automation/configure",
    ),
    converts(
      "delivery_claims.rs",
      "DeliveryClaim.mode l16 String checked against auto|steer|queue l78; event deliveryAttempt json! l113",
    ),
    none(
      "delivery_effect_transition.rs",
      "allows_completion l13 checks AcceptedDeliveryEffect against SubmissionEffect/PeerWriteEffect of agent-automation RouteEffectEvidence; one correspondence over existing types, no second encoding; cfg(test) mod tests l121",
    ),
    defines(
      "delivery_inspection.rs",
      "DeliveryRecord l7 (serde) with mode: String l12 read from the delivery_mode column; part of the stored WakeMutationResult",
    ),
    converts(
      "delivery_outcomes.rs",
      "event deliveryAttempt json! l150; DeliveryResult l11 is an in-process input",
    ),
    none(
      "delivery_preparation.rs",
      "persists caller RouteEffectEvidence into the typed DeliveryAttempt (l57); no new shape",
    ),
    converts("delivery_recovery.rs", "event deliveryAttempt json! l106"),
    defines(
      "external_operation_receipts.rs",
      "ExternalOperationRecord l7 stored external-operation record (a persistence contract, projected to the public OperationSnapshot in collaboration-service) with method and status Strings; method literal schedule/prepare l47, l68, l84, l90",
    ),
    converts(
      "instruction_repository.rs",
      "local-effects literal l53 and method literal instruction/create l26, l51; event body is the typed InstructionDocument JSON l57",
    ),
    none(
      "instruction_revision_read.rs",
      "InstructionRevisionRecord l5 (no serde) decoded from typed columns",
    ),
    converts(
      "instruction_updates.rs",
      "local-effects literal l66 and method literal instruction/update l35, l64",
    ),
    none("lib.rs", "module declarations and re-exports"),
    converts(
      "local_operation_receipts.rs",
      "LocalOperation.method l8 bare &str; record() writes the local-effects literal l53",
    ),
    none(
      "message_drop_guard.rs",
      "migration-ledger check of legacy message tables (l32-56); SQLite only",
    ),
    defines(
      "operation_receipt_read.rs",
      "StoredOperationState l5 / StoredOperationRecord l24 untyped mirror of OperationState; status strings mapped by hand l51-57",
    ),
    defines(
      "owed_push_operation.rs",
      "PushOperationId l12 second operation-id newtype; PushOperation l46 and OwedPushOperation l132 wrap existing PushRecordDraft/DeliveryReceipt for the private owed_push_operations queue; l101 states the schedule-run settlement state/outcome rule",
    ),
    none(
      "owed_push_operations.rs",
      "SQL encode/decode of OwedPushOperation (l14-31, l118) defined in owed_push_operation.rs",
    ),
    converts(
      "preparation_completion.rs",
      "PreparedThread l12 carries the thread address as three Strings (l17-19); event scheduleEdit json! l93; method literal schedule/prepare l31, l49",
    ),
    converts(
      "preparation_failures.rs",
      "writes caller-typed evidence and error JSON with the method literal schedule/prepare l26; PreparationFailureRecord l10 has no serde",
    ),
    converts(
      "remote_wake_requests.rs",
      "target SessionRef as JSON text is the SQL equality key (l97, l114-116); stores collaboration_mail_protocol WakeBody and WakeSnapshot JSON",
    ),
    converts("run_admission.rs", "event stateChange raw string literal l97"),
    converts(
      "run_completion_state.rs",
      "event stateChange json! l100; native turn id compared as a string l60",
    ),
    none(
      "run_dispatch_state.rs",
      "validates RouteEffectEvidence submission states (l39-96) and writes typed RunExecutionEvidence; no second encoding",
    ),
    none("run_inspection.rs", "decodes the typed agent-automation RunRecord from columns (l19-89)"),
    none("run_inventory.rs", "run occupancy query; RunPhase decoded from its serde string l20"),
    converts("run_preparation_state.rs", "event stateChange json! l89"),
    defines(
      "run_stop_state.rs",
      "RunStopIdentity l6 with NativeTurn(String) compared by as_str with RunRecord.native_turn_id l47",
    ),
    converts(
      "run_submission_outcomes.rs",
      "RunSubmissionOutcome l11 to PushDeliveryState table l117-130; turn_id String l13 compared l146; l147-154 compares same-type targets and generations through serde_json::to_value",
    ),
    none(
      "run_uncertainty_state.rs",
      "stores typed NativeEffectEvidence into RunExecutionEvidence (l34); no new shape",
    ),
    defines(
      "schedule_admission_queries.rs",
      "BindingAddress l4 carries the thread address as three bare &str",
    ),
    converts("schedule_evaluation.rs", "event stateChange json! l67"),
    none(
      "schedule_export.rs",
      "assembles agent_automation PortableSchedulePackage (l68), whose JSONL encoding is inventoried at agent-automation portable_schedule_encoding.rs:12",
    ),
    converts(
      "schedule_import.rs",
      "event scheduleEdit json! l121; method literal schedule/import l38, l138",
    ),
    defines(
      "schedule_inspection.rs",
      "ScheduleInspection l7 stored as the schedule operation result, decoded later into MCP ScheduleSnapshot",
    ),
    converts(
      "schedule_mutations.rs",
      "event scheduleEdit json! l110; method literals schedule/update|enable|disable l35-37; ScheduleEdit l8 serializes only into canonical request bytes l39",
    ),
    converts(
      "schedule_repository.rs",
      "local-effects literal l96 and method literal schedule/create l56, l95",
    ),
    reads(
      "schedule_worker_inventory.rs",
      "SQL json_extract of SummaryAttempt $.deadlineAtMs and $.phase l42",
    ),
    none(
      "schema_definition_catalogue.rs",
      "TABLE_SPECS l20 and INDEX_SPECS l83: SQLite schema text only",
    ),
    none(
      "schema_initialization.rs",
      "SQLx migration sequencing (l26-100); cfg(test) mod tests l139",
    ),
    none(
      "schema_validation.rs",
      "SQLite structural validation and DDL tokenizer; cfg(test) mod tests l442",
    ),
    none(
      "summary_admission.rs",
      "builds a typed agent-automation SummaryAttempt (make_attempt l77); no second encoding",
    ),
    converts(
      "summary_completion.rs",
      "event stateChange json! l57; SummaryCompletion.native_turn_id l9 String compared l36",
    ),
    none(
      "summary_progress.rs",
      "copies SummaryProgress fields into the typed SummaryAttempt (l49-53); native_turn_id l12 passes through as Option<String>",
    ),
    converts(
      "summary_recovery.rs",
      "event summaryAttempt json! l93; method literals run/summaryRetry|run/summarySkip l32-33",
    ),
    defines(
      "thread_binding_repository.rs",
      "ThreadBindingClaim l5 carries the thread address as three Strings l7-9",
    ),
    none(
      "wake_limits.rs",
      "wake timing limits returning collaboration_protocol WakeRefusal values (l32-58); no new shape",
    ),
    converts(
      "wake_slot_upgrade.rs",
      "target SessionRef JSON string as HashSet key l60; event stateChange l99; legacy createdBy unrecorded insert l121; stored wake operation results rewritten by JSON pointer l143",
    ),
    converts(
      "wakeup_evaluation.rs",
      "events expired l82, coalesced l116, fired l155 hand-built; writes DurableMessage::delivery_mode string l145",
    ),
    none(
      "wakeup_listing.rs",
      "WakeListPosition l7 serde cursor position encoded by collaboration-service; typed WakeRecord reads",
    ),
    defines(
      "wakeup_mutations.rs",
      "WakeMutationResult l25 with untyped RetainedDelivery l22 stored as the wake mutation result; event {kind} l171; method literals wake/pause|resume|cancel l41-43",
    ),
    none(
      "wakeup_observation.rs",
      "WakeTransition l6 returns wake event rows (kind String, body Value) to an in-process waiter; nothing parsed here",
    ),
    converts(
      "wakeup_repository.rs",
      "delivery-mode literal check l28; SQL json_extract of SessionRef fields l151; WakeCreator session JSON vs target JSON l166; events l101, l187; method literal wake/send l49, l118",
    ),
    none(
      "wakeup_worker_inventory.rs",
      "bounded due-wake and eligible-delivery id queries; no message shape",
    ),
  ],
  sendSites: [],
};
