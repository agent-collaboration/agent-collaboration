import type { InventoryRow, Owner } from "../schema-audit-types.ts";

const A = "crates/automation-storage/src/";
const storage: Owner[] = [{ crate: "automation-storage" }];
/** Stored operation receipts and automation events reach MCP operation_show and automation_events through collaboration-service. */
const mcpThroughService: Owner[] = [
  { part: "N11" },
  { crate: "automation-storage" },
  { crate: "collaboration-service" },
];

/** A hand-built automation event body, decoded by its "kind" key in collaboration-service automation_event_projection.rs:16 into AutomationEventDetails. */
function eventBody(file: string, line: number, defines: string): InventoryRow {
  return {
    file: `${A}${file}`,
    line,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines,
    usedBy: mcpThroughService,
    duplicateGroup: "automation-event-hand-json",
    disposition: "duplicate",
    fixedBy: "D2",
  };
}

/**
 * The only producer of one automation event variant: the hand-built body is that variant's one
 * encoding (read back by its "kind" in collaboration-service automation_event_projection.rs:16),
 * not a copy of another variant in the same event family.
 */
function uniqueEventVariant(file: string, line: number, defines: string): InventoryRow {
  return {
    file: `${A}${file}`,
    line,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines,
    usedBy: mcpThroughService,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  };
}

/** The local-effects literal {"kind":"local","mutation":"committed"} (OperationEffects::Local, operation_inspection_contract.rs:22) and a method literal. */
function localReceipt(file: string, line: number, defines: string): InventoryRow {
  return {
    file: `${A}${file}`,
    line,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines,
    usedBy: mcpThroughService,
    duplicateGroup: "operation-receipt-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  };
}

function identity(file: string, line: number, defines: string, malformed = false): InventoryRow {
  return {
    file: `${A}${file}`,
    line,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines,
    usedBy: storage,
    duplicateGroup: "session-ref-conversion",
    disposition: malformed ? "malformed" : "duplicate",
    fixedBy: "D2",
  };
}

export const rows: InventoryRow[] = [
  {
    file: `${A}operation_receipt_read.rs`,
    line: 5,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "StoredOperationState {admitted, inProgress, uncertain, succeeded, failed} with untyped serde_json::Value effects/result/failure is the stored transaction record, a distinct persistence contract that collaboration-service operation_receipt_projection.rs:13-170 projects to public OperationSnapshot/OperationState (not a copy of it); the defect is the vocabulary spelled by hand here: StoredOperationRecord.method is a bare String (L26) and the status column strings admitted/inProgress/uncertain/succeeded/failed are matched to variants by hand (L51-57)",
    usedBy: mcpThroughService,
    duplicateGroup: "operation-receipt-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${A}external_operation_receipts.rs`,
    line: 7,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      'ExternalOperationRecord {operationId, method: String, resourceId, status: String, evidence, result, failure: Value, admittedAtMs}: the stored external-operation record, a persistence contract distinct from the public OperationSnapshot; the defect is its hand-spelled vocabulary: method and status are bare Strings, and the method literal "schedule/prepare" (L47, L68, L84, L90; also preparation_failures.rs:26) spells AutomationOperationMethod::SchedulePrepare (operation_outcome_contract.rs:18) by hand',
    usedBy: mcpThroughService,
    duplicateGroup: "operation-receipt-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${A}local_operation_receipts.rs`,
    line: 8,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      'LocalOperation.method is a bare &str and record() writes the effects literal {"kind":"local","mutation":"committed"} (L53); callers spell AutomationOperationMethod names by hand: schedule_import.rs:38/138, schedule_mutations.rs:35-37, summary_recovery.rs:32-33, wakeup_mutations.rs:41-43, wakeup_repository.rs:49/118',
    usedBy: mcpThroughService,
    duplicateGroup: "operation-receipt-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${A}preparation_failures.rs`,
    line: 26,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "record_preparation_failure matches the receipt row by the method literal method_name='schedule/prepare' and the status literals failed/uncertain and admitted/inProgress/uncertain in SQL (L22-26), spelling AutomationOperationMethod::SchedulePrepare and the operation status vocabulary by hand, as external_operation_receipts.rs:7 does",
    usedBy: mcpThroughService,
    duplicateGroup: "operation-receipt-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  localReceipt(
    "instruction_repository.rs",
    53,
    'create_instruction writes the local-effects literal and the method literal "instruction/create" (L26, L51) into operation_receipts by hand',
  ),
  localReceipt(
    "instruction_updates.rs",
    66,
    'update_instruction writes the local-effects literal and the method literal "instruction/update" (L35, L64) into operation_receipts by hand',
  ),
  localReceipt(
    "schedule_repository.rs",
    96,
    'create_schedule writes the local-effects literal and the method literal "schedule/create" (L56, L95) into operation_receipts by hand',
  ),
  {
    file: `${A}configuration_receipts.rs`,
    line: 35,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      'json! effects {"kind":"configuration","fileState","intended"} (L35, L80, L137) hand-build OperationEffects::Configuration (operation_inspection_contract.rs:29); method literal "automation/configure" (L36, L53, L76, L81, L89, L112, L141); status literals inProgress/uncertain/succeeded',
    usedBy: mcpThroughService,
    duplicateGroup: "operation-receipt-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${A}schedule_inspection.rs`,
    line: 7,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "ScheduleInspection {record, activeRunId, waitingRunId}: the stored schedule operation result serialized into operation_receipts.final_result_json (schedule_mutations.rs:119, schedule_import.rs:132, preparation_completion.rs:101); a persistence contract that collaboration-service schedule_projection projects to the public MCP ScheduleSnapshot, not a copy of it",
    usedBy: mcpThroughService,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${A}wakeup_mutations.rs`,
    line: 25,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "WakeMutationResult {observedAtMs, wake, discarded, retained}: the durable wake/pause|resume|cancel transaction result, with RetainedDelivery = DeliveryRecord<Value, Value, Value> (L22) holding adapter-owned payloads; collaboration-service wakeup_projection.rs:65-83 projects it to the public collaboration-protocol WakeMutationResult (WakeSnapshot, discarded ids, projected DeliveryInspection), a different contract despite the shared name",
    usedBy: mcpThroughService,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${A}delivery_inspection.rs`,
    line: 12,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "DeliveryRecord.mode is a String holding the delivery mode read straight from the delivery_mode column (L71), not MessageDelivery",
    usedBy: mcpThroughService,
    duplicateGroup: "delivery-mode-mapping",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${A}delivery_claims.rs`,
    line: 16,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      'DeliveryClaim.mode is a String checked against the literals "auto"|"steer"|"queue" by hand (L78)',
    usedBy: storage,
    duplicateGroup: "delivery-mode-mapping",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${A}wakeup_repository.rs`,
    line: 28,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      'create_wakeup checks DurableMessage::delivery_mode() against the literals "auto"|"steer"|"queue" by hand',
    usedBy: storage,
    duplicateGroup: "delivery-mode-mapping",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${A}owed_push_operation.rs`,
    line: 12,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "PushOperationId(String): a second operation-identity newtype that only borrows agent_automation::OperationId for generation and validation (L16, L28) and is stored as its own string; an operation identity, not a SessionRef",
    usedBy: storage,
    duplicateGroup: null,
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${A}owed_push_operation.rs`,
    line: 101,
    owner: "router-vocabulary",
    kind: "content-parser",
    defines:
      "PushOperation::validate states which DeliveryOutcome each PushDeliveryState admits for a schedule-run settlement (delivered, outcomeUnknown, and pending for a rejected outcome); a schedule-run-specific policy, deliberately different from the ordinary Push record's rejected state (collaboration-protocol push_record.rs:350-416), so not a copy of that table",
    usedBy: storage,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${A}run_submission_outcomes.rs`,
    line: 117,
    owner: "router-vocabulary",
    kind: "content-parser",
    defines:
      "record_run_submission_inner checks RunSubmissionOutcome against the PushDeliveryState of the same schedule-run settlement (accepted/providerAdmitted/peerWritten to delivered, unknown to outcomeUnknown or delivered when the route reports acceptance, rejected to pending); the schedule-run policy, not a copy of the ordinary Push state table (push_record.rs:350-416)",
    usedBy: storage,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${A}run_stop_state.rs`,
    line: 7,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "the Codex turn id is a bare String in RunStopIdentity::NativeTurn and compared by as_str with RunRecord.native_turn_id (L47, run_completion_state.rs:60); also RunSubmissionOutcome::Accepted.turn_id (run_submission_outcomes.rs:13, compared L146) and SummaryCompletion.native_turn_id (summary_completion.rs:9, compared L36); a native turn identity, not a SessionRef",
    usedBy: storage,
    duplicateGroup: null,
    disposition: "duplicate",
    fixedBy: "D2",
  },
  identity(
    "thread_binding_repository.rs",
    5,
    "ThreadBindingClaim carries the native thread address as three bare strings service_id, endpoint_id, thread_id (L7-9), checked only for length and NUL (L30)",
  ),
  identity(
    "schedule_admission_queries.rs",
    4,
    "BindingAddress carries the native thread address as three bare &str service_id, endpoint_id, thread_id",
  ),
  identity(
    "preparation_completion.rs",
    17,
    'PreparedThread carries service_id, endpoint_id, thread_id as Strings into ThreadBindingClaim (L65-73); method literal "schedule/prepare" (L31, L49)',
  ),
  identity(
    "remote_wake_requests.rs",
    114,
    "the target SessionRef serialized to JSON text is the SQL equality key target_json=? (stored L97, matched L114-116)",
    true,
  ),
  identity(
    "wake_slot_upgrade.rs",
    60,
    "the wake message target SessionRef serialized to a JSON string is the duplicate-target HashSet key during upgrade",
    true,
  ),
  identity(
    "wakeup_repository.rs",
    151,
    "replace_occupant matches the occupying wake by SQL json_extract of SessionRef fields $.message.target.endpoint.serviceId, .endpointId and .sessionId against the JSON-serialized target",
    true,
  ),
  identity(
    "wakeup_repository.rs",
    166,
    "WakeCreator::Session(SessionRef) serialized to JSON is compared with the JSON-serialized TMessage target (L149) to decide is_target: two types compared through JSON text",
    true,
  ),
  {
    file: `${A}attempt_history_read.rs`,
    line: 240,
    owner: "router-vocabulary",
    kind: "content-parser",
    defines:
      "decode_attempt reads attemptId and startedAtMs from the stored attempt JSON by key, SQL json_extract reads $.attempt.attemptId and $.attempt.startedAtMs (L171-172, L219), and StoredAttemptRecord (L23) returns body and receipt as Value for MCP delivery/summary attempt pages; a read of this crate's own stored attempt shape, not a copy of another parser",
    usedBy: mcpThroughService,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${A}schedule_worker_inventory.rs`,
    line: 42,
    owner: "router-vocabulary",
    kind: "content-parser",
    defines:
      "scheduler_next_due_at_ms reads the stored SummaryAttempt fields $.deadlineAtMs and $.phase out of summary_attempt_json with SQL json_extract; a read of the stored summary shape, a different stored shape from the attempt history read (attempt_history_read.rs:240)",
    usedBy: storage,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${A}wake_slot_upgrade.rs`,
    line: 121,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      'legacy_definition inserts createdBy {"kind":"unrecorded"} (WakeCreator::Unrecorded) into a stored wake definition Value before typed decoding',
    usedBy: storage,
    duplicateGroup: "legacy-stored-decoders",
    disposition: "boundary-encoding",
  },
  {
    file: `${A}wake_slot_upgrade.rs`,
    line: 143,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "upgrade_operation_answers rewrites stored wake/send|pause|resume|cancel results (method literals L132) at JSON pointer /definition or /wake/definition chosen by method name, then re-checks them as WakeRecord or WakeMutationResult",
    usedBy: storage,
    duplicateGroup: "legacy-stored-decoders",
    disposition: "boundary-encoding",
  },
  uniqueEventVariant(
    "configuration_receipts.rs",
    146,
    'complete_configuration event {"kind":"configurationChange","configuration"}: the only producer of this variant (read at automation_event_projection.rs:56)',
  ),
  eventBody(
    "delivery_claims.rs",
    113,
    'claim_delivery event {"kind":"deliveryAttempt","attempt","receipt"} (event kind attemptArchived)',
  ),
  eventBody(
    "delivery_outcomes.rs",
    150,
    'complete_delivery event {"kind":"deliveryAttempt","attempt","receipt"} (event kind attemptCompleted)',
  ),
  eventBody(
    "delivery_recovery.rs",
    106,
    'recover_interrupted_deliveries event {"kind":"deliveryAttempt","attempt"} (event kind dispatchInterrupted)',
  ),
  eventBody(
    "run_admission.rs",
    97,
    'admit_waiting_run event as a raw string literal {"kind":"stateChange","before":"waiting","after":"preparing"}',
  ),
  eventBody(
    "run_completion_state.rs",
    100,
    'complete_run_settlement event {"kind":"stateChange","before","after"} with the run phase spelled as a literal (L87-89)',
  ),
  eventBody(
    "run_preparation_state.rs",
    89,
    'fail_run_preparation event {"kind":"stateChange","before","after":"preparationFailed"}',
  ),
  eventBody(
    "schedule_evaluation.rs",
    67,
    'enqueue_due_run event {"kind":"stateChange","before":null|"waiting","after":"waiting"}',
  ),
  eventBody(
    "summary_completion.rs",
    57,
    'complete_summary event {"kind":"stateChange","before":"summaryRunning","after":"finished"}',
  ),
  uniqueEventVariant(
    "summary_recovery.rs",
    93,
    'recover_summary event {"kind":"summaryAttempt","attempt"} (event kind summaryAttemptArchived): the only producer of this variant (read at automation_event_projection.rs:43)',
  ),
  eventBody(
    "schedule_mutations.rs",
    110,
    'mutate_schedule event {"kind":"scheduleEdit","before","after"}',
  ),
  eventBody(
    "schedule_import.rs",
    121,
    'import_schedule event {"kind":"scheduleEdit","before","after","sourceChangeId"}',
  ),
  eventBody(
    "preparation_completion.rs",
    93,
    'complete_thread_preparation event {"kind":"scheduleEdit","before","after"}',
  ),
  eventBody(
    "wake_slot_upgrade.rs",
    99,
    'upgrade event {"kind":"stateChange","before","after":"expired"} under event kind upgradeDuplicate|upgradeLimit',
  ),
  eventBody(
    "wakeup_evaluation.rs",
    82,
    'evaluate_wakeup expiry event {"kind":"expired"}, the same expired body mutate_wakeup builds independently (wakeup_mutations.rs:76,171)',
  ),
  uniqueEventVariant(
    "wakeup_evaluation.rs",
    116,
    'evaluate_wakeup coalesced event {"kind":"coalesced","deliveryId","throughMs"}: the only producer of the wake coalesced variant',
  ),
  uniqueEventVariant(
    "wakeup_evaluation.rs",
    155,
    'evaluate_wakeup fired event {"kind":"fired","fire"}: the only producer of the wake fired variant',
  ),
  eventBody(
    "wakeup_mutations.rs",
    171,
    'mutate_wakeup event {"kind":<event kind>} reusing the event-kind string (paused, cancelled, expired, resumed, finished, finishedWithoutFiring) as the body kind',
  ),
  eventBody(
    "wakeup_repository.rs",
    101,
    'create_wakeup event {"kind":"stateChange","before":null,"after":status}',
  ),
  eventBody(
    "wakeup_repository.rs",
    187,
    'replace_occupant event {"kind":"stateChange","before","after":"cancelled"} under event kind replaced',
  ),
];
