import type { LedgerSection } from "../schema-audit-types.ts";

const MCP_TAP = "crates/collaboration-mcp/src/mcp_server_node_tap_test.rs";

export const section: LedgerSection = {
  root: "crates/collaboration-service",
  files: [
    {
      file: "crates/collaboration-service/src/application_fixture.rs",
      disposition: { kind: "none" },
      note: 'test-support builder (cfg(any(test, feature = "test-support")) at lib.rs:17); ApplicationFixture l6 only wires operation families, no message shape',
    },
    {
      file: "crates/collaboration-service/src/attempt_history_projection.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "summary l90 converts agent_automation SummaryPhase (l114) and CessationEvidence (l118) into protocol SummaryInspection state/cessation by serde_json to_value/from_value; delivery l7 is a typed AttemptOutcome to DeliveryEvidence match",
    },
    {
      file: "crates/collaboration-service/src/automation_collection_cursor.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "CollectionCursor l8 with encode l20 / decode l25: the canonical opaque base64url-JSON page cursor codec for automation collections (boundary-encoding inventory row)",
    },
    {
      file: "crates/collaboration-service/src/automation_collection_projection.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "project_record l6 serializes typed Instruction/Schedule/Run/Delivery snapshots and RevisionRecord into serde_json::Value (l18-76) for page measuring; typed_records (automation_inspection_operations.rs:393) decodes them back",
    },
    {
      file: "crates/collaboration-service/src/automation_event_cursor.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "encode l4 / decode l14: the canonical JSON-tuple automation-events cursor codec (boundary-encoding inventory row); independent copies at automation_operations.rs:97, wakeup_projection.rs:34, wakeup_subscription.rs:79 are duplicate rows",
    },
    {
      file: "crates/collaboration-service/src/automation_event_projection.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "project l9 decodes the stored automation event body by hand (body.kind string dispatch l16, required fields l146) into protocol AutomationEvent; wake event_kind strings mapped to StateChange.after strings l83-89",
    },
    {
      file: "crates/collaboration-service/src/automation_inspection_failure.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "storage l21 maps automation_storage StorageError onto AutomationInspectionFailureKind/NextAction (catch-all l36); invalid l6, unavailable l55 build typed failures",
    },
    {
      file: "crates/collaboration-service/src/codex_session_inventory.rs",
      disposition: { kind: "sends", owners: ["codex", "router-vocabulary"] },
      note: "sends thread/loaded/list (l474), thread/read (l490), thread/turns/list (l612) via request_validated; decodes Codex replies by hand (l481-551, l623 ThreadTurnsPage l22, l633 error text); builds Router session records as json! l440, l540 and the NativeSessionListResult as json! l652 then from_value l659; thread id to SessionRef l384, l515",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/automation_history_operations.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "deliveryId/runId carried through hand json! filters l30, l50 and read back with serde_json::from_value l326, l342; projected attempts serialized to Value l334, l350 and decoded by typed_records l191",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/automation_inspection_operations.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "typed_records l393 decodes page records from serde_json::Value; json! filters l41-118 only feed the cursor digest; delivery_reconcile l250 and run_reconcile l278 reach native evidence through collaboration_runtime ports",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/automation_operations.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "automation_status l83 hand-encodes earliest_retained_event_cursor as a JSON tuple string l97 (second copy of automation_event_cursor.rs:5); StorageError mapped to RunFailureKind l368 and InstructionFailureKind l475",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/conversation_operations.rs",
      disposition: { kind: "none" },
      note: "typed ConversationOperationFailure built in local_failure l594; provider calls go through the ProviderConversationBackend port (its implementers own the ACP/app-server sends)",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/interaction_operations.rs",
      disposition: { kind: "converts", owners: ["mcp", "router-vocabulary"] },
      note: "InteractionFailure::payload l148 built by payload.insert l158-168 plus ApprovalDecisionError detail l164; question_record_view l219 converts requester SessionRef l229 and question fields l236 by serde_json round trip; InteractionHistoryError mapped l201",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/journal_operations.rs",
      disposition: { kind: "converts", owners: ["mcp"] },
      note: "JournalFailure Serialize l185 hand-builds failure bodies (json! l191-196); JournalError mapped onto JournalFailure l60-76, l110-117",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/message_operations.rs",
      disposition: { kind: "defines", owners: ["mcp", "router-vocabulary"] },
      note: "defines MCP failure body MessageFailure l463 {kind,stage,message,existing} with MessageFailureKind l473 and MessageFailureStage l497; MailRefusal/PushDeliveryFailure mapped l384; MailItemId to PushId l93",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/message_status_operations.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "public_status l342 copies fleet_sync_protocol DeliveryStatus/DeliveryRefusal onto collaboration_protocol RemoteDeliveryStatus; PushId to MailItemId to fleet ItemId l105-112; caller SessionRef to message_board SessionRef l246",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/mod.rs",
      disposition: { kind: "none" },
      note: "module wiring and re-exports; CollaborationApplication l69 holds the operation families",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/provider_observation_operations.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary", "mcp"] },
      note: "attach l222 converts protocol SessionRef to message_board SessionRef by serde_json to_value/from_value l253; event_value l268 wraps a SessionEvent as {sequence,event} (json! l269); resync markers l92, l135; EventTooLarge l285; ObservationFailure Serialize l315 hand-builds {kind,stage,message} (l320)",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/schedule_operations.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "schedule definitions converted by serde_json round trip l49, l101; l250 hand-builds a jsonrpc 2.0 schedule/import envelope only to measure the response byte budget, never sent; StorageError mapped onto ScheduleFailure l412",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/session_native_control.rs",
      disposition: { kind: "sends", owners: ["codex"] },
      note: "native_call l273 and rename l141 send thread/read (params json! l51, l172), turn/interrupt (l112) and thread/name/set (l196) through NativeProtocolConnection::request_validated (l174, l193, l209, l312); replies decoded by hand l74-80, l125, l188, l223, l416; classify_native_call_failure l402",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/session_operations.rs",
      disposition: { kind: "none" },
      note: "delegates to NativeSessionControl and the inventories; installation_show l60 returns typed InstallationIdentity carrying FleetLinkRecord unchanged",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/wake_operations.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "wake_send_local converts request timing/expiry by serde_json round trip l93-94; mutate maps StorageError onto WakeFailureReason l374 (second copy of failure.rs:68); json!({}) l184 only feeds the cursor digest",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/wake_operations/failure.rs",
      disposition: { kind: "converts", owners: ["mcp"] },
      note: "invalidField rejection data built with json! l182; storage_reason l68 maps StorageError onto WakeFailureReason; wake_failure l33 derives stage/next action/message",
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/wake_operations/remote.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "convert l14 (serde_json to_value/from_value) turns operation ids, wakeup ids, requester, SavedMessage (target SessionRef), timing and deadline into collaboration_mail_protocol types (l72-81, l128, l196-197, l255-257) and remote snapshots, results and failure reasons back (l100, l140, l222, l269, l383, l400, l406-407)",
    },
    {
      file: "crates/collaboration-service/src/collaboration_rejection.rs",
      disposition: { kind: "defines", owners: ["mcp"] },
      note: "PublishedRejection l51 {code,message,data}: the JSON-RPC error object every family's failure becomes (codes l60-66), consumed by collaboration-mcp application_tool_results.rs:21; CollaborationRejectionReason l9 is not published",
    },
    {
      file: "crates/collaboration-service/src/conversation_operation_projection.rs",
      disposition: { kind: "none" },
      note: "conversation_operation_snapshot l5: typed field copy ProviderOperationRecord to ConversationOperationSnapshot (queue_state and input_id set None l17-18); no serde round trip",
    },
    {
      file: "crates/collaboration-service/src/delivery_projection.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "snapshot l8 maps the stored mode string auto/steer/queue onto MessageDelivery l15; DeliveryStatus to DeliveryDisposition l21 and AttemptOutcome to DeliveryEvidence l30 typed",
    },
    {
      file: "crates/collaboration-service/src/instruction_projection.rs",
      disposition: { kind: "none" },
      note: "snapshot l5: typed InstructionDocument to InstructionSnapshot with RFC 3339 times",
    },
    {
      file: "crates/collaboration-service/src/inventory_failures.rs",
      disposition: { kind: "converts", owners: ["mcp"] },
      note: "NativeSessionFailure Serialize l93 hand-builds failure bodies (json! l100-129); ProviderInventoryFailure Serialize l164 hand-builds {kind,stage,source,reason,message} (l171-179)",
    },
    {
      file: "crates/collaboration-service/src/lib.rs",
      disposition: { kind: "none" },
      note: "module wiring; re-exports codex_acp_adapter::ACP_SCHEMA_DIGEST l37 (digest owned by codex-acp-adapter)",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_projection_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/mail_delivery_projection_tests.rs:6 (that module is #[cfg(test)] at lib.rs:81)",
      },
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_projection_intake.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/mail_delivery_projection_fixture.rs:472 (inside the cfg(test) chain from lib.rs:81)",
      },
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runner_probe.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/mail_push_delivery_tests.rs:5 (that module is #[cfg(test)] at mail_push_delivery.rs:150)",
      },
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "mail recipient address to SessionRef by try_into l448; fleet ItemId to PushId through its UUID string l478; service id UuidIdentity to message_board::ServiceId l277; the fleet_sync_delivery engine itself is central mail custody",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/custody_backfill.rs",
      disposition: { kind: "none" },
      note: "fleet_sync_delivery startup backfill (central fleet mail custody, not one of the four protocols): admit_existing_record_removals l14, backfill_submitters l39",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/held_readiness.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "PushId to fleet ItemId by Uuid::parse_str l46; held mail release through fleet_sync_delivery Inbox l51",
    },
  ],
  sendSites: [
    {
      file: "crates/collaboration-service/src/collaboration_application/session_native_control.rs",
      line: 312,
      edge: "managed-codex",
      sends:
        "native_call transport line for thread/read (caller l53) and turn/interrupt (caller l114)",
      owner: { part: "N10" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "thread/read from this helper is recorded by no tap (validated_native_request_node_tap_test records its own caller's {threadId}); turn/interrupt is covered on its caller row l114",
      },
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/session_native_control.rs",
      line: 53,
      edge: "managed-codex",
      sends: "thread/read {threadId, includeTurns:false} from codex_session_inspect",
      owner: { part: "N10" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "the only MCP to N10 tap (node_tap_native_control_tool_routes_to_codex_client) calls turn_interrupt; no tap drives session_inspect to the managed Codex wire",
      },
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/session_native_control.rs",
      line: 114,
      edge: "managed-codex",
      sends: "turn/interrupt {threadId, turnId} from codex_turn_interrupt",
      owner: { part: "N10" },
      origin: "product",
      coverage: {
        kind: "tap",
        testFile: MCP_TAP,
        cases: ["node_tap_native_control_tool_routes_to_codex_client"],
      },
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/session_native_control.rs",
      line: 174,
      edge: "managed-codex",
      sends: "thread/read {threadId, includeTurns:false} before rename",
      owner: { part: "N10" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason: "N10 tap does not record includeTurns:false params from this caller",
      },
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/session_native_control.rs",
      line: 193,
      edge: "managed-codex",
      sends: "thread/name/set {threadId, name}",
      owner: { part: "N10" },
      origin: "product",
      coverage: { kind: "uncovered", reason: "no tap case records thread/name/set" },
    },
    {
      file: "crates/collaboration-service/src/collaboration_application/session_native_control.rs",
      line: 209,
      edge: "managed-codex",
      sends: "thread/read verification after rename",
      owner: { part: "N10" },
      origin: "product",
      coverage: { kind: "uncovered", reason: "N10 tap does not record this caller" },
    },
    {
      file: "crates/collaboration-service/src/codex_session_inventory.rs",
      line: 474,
      edge: "managed-codex",
      sends: "thread/loaded/list {limit, cursor}",
      owner: { part: "N10" },
      origin: "product",
      coverage: { kind: "uncovered", reason: "no tap case records thread/loaded/list" },
    },
    {
      file: "crates/collaboration-service/src/codex_session_inventory.rs",
      line: 490,
      edge: "managed-codex",
      sends: "thread/read {threadId, includeTurns:false}",
      owner: { part: "N10" },
      origin: "product",
      coverage: { kind: "uncovered", reason: "N10 tap records thread/read with {threadId} only" },
    },
    {
      file: "crates/collaboration-service/src/codex_session_inventory.rs",
      line: 612,
      edge: "managed-codex",
      sends: "thread/turns/list",
      owner: { part: "N10" },
      origin: "product",
      coverage: { kind: "uncovered", reason: "no tap case records thread/turns/list" },
    },
  ],
};
