import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
  {
    file: "crates/collaboration-service/src/collaboration_application/wake_operations/remote.rs",
    line: 100,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "convert back from collaboration_mail_protocol results: WakeSnapshot l100 l140 l383, mutation result l222, wait answer l269, WakeFailureReason l400 l406, LocalMutationState l407",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_rejection.rs",
    line: 51,
    owner: "mcp",
    kind: "rust-type",
    defines:
      "PublishedRejection {code, message, data}: the application's typed publication intermediate with codes INVALID_PARAMS -32602, OPERATION_FAILED -32050, NOT_FOUND -32002, INTERNAL -32603 (l51-66); collaboration-mcp application_tool_results.rs:21-43,67-91 converts it into AdapterOperationFailure or a structured domain error, so it is not itself the MCP wire error object",
    usedBy: [
      {
        part: "N11",
      },
      {
        crate: "collaboration-mcp",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/delivery_projection.rs",
    line: 15,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "stored delivery mode string auto/steer/queue to MessageDelivery (a string copy of MessageDelivery's serde names)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "delivery-mode-strings",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/inventory_failures.rs",
    line: 100,
    owner: "mcp",
    kind: "hand-json",
    defines:
      "NativeSessionFailure bodies: invalidRequest {kind,message} l100, refused {kind,stage,message} l106, nativeRejected {kind,stage,message,reason,nextAction,nativeCode?} l114-123, nameMismatch {kind,requested,effective,stage} l129",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/inventory_failures.rs",
    line: 164,
    owner: "mcp",
    kind: "hand-json",
    defines:
      "ProviderInventoryFailure bodies: {kind:invalidRequest,message} l171, {kind:invalidNativeInventory,stage:discovery,source,reason,message} l176, {kind,stage:discovery,message} l179",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime.rs",
    line: 277,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines: "with_limits: service id UuidIdentity to message_board::ServiceId through its string",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "service-identity-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime.rs",
    line: 448,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "on_received: mail recipient address converted to the poll target SessionRef by try_into",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime.rs",
    line: 478,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines: "on_outcome: fleet ItemId to PushId through its UUID string",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "fleet-item-identity",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/frozen_cancel_outcome.rs",
    line: 56,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "local frozen cancel: DeliveryCancel<BoardDeliveryTypes> (recipient address carries the SessionRef) converted to cancel_local's type by serde_json to_value then from_value",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/frozen_cancel_outcome.rs",
    line: 158,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "remote frozen cancel: the same DeliveryCancel converted for enqueue_project_cancel by serde_json to_value then from_value",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/frozen_handoff_status.rs",
    line: 11,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines: "await_native_outcome_commit: fleet ItemId to PushId through its UUID string",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "fleet-item-identity",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/held_readiness.rs",
    line: 46,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines: "release_held_mail: PushId to fleet ItemId by Uuid::parse_str",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "fleet-item-identity",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/local_admission.rs",
    line: 149,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "fleet ItemId / MailItemId / PushId / MailItemRef conversions. local_admission.rs:149 and :167 use the one shared typed conversion TryFrom<PushId> for MailItemId (collaboration-protocol mail_boundary_conversions.rs:72-75), not a codec of their own; the conversions through uuid strings by hand are local_cancellation.rs:104, :148; project_admission.rs:71, :207; poll_handover.rs:31; remote_status.rs:162; frozen_handoff_status.rs:11; remote_calls.rs:45; project_cancellation.rs:64; project_admission/membership_mail_cancels.rs:41 (PushId reaches MCP)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/pushed_mail.rs",
    line: 37,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "runtime_test_pushed_item (feature runtime-test-support) re-types the resolved DeliveryItem<RouterDeliveryTypes> as DeliveryItem<BoardDeliveryTypes> by to_value then from_value, carrying the SessionRef recipient",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/pushed_mail.rs",
    line: 52,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "accept_pushed_mail re-types DeliveryEnvelope<BoardDeliveryTypes> as DeliveryEnvelope<RouterDeliveryTypes> by serde_json to_value then from_value; the envelope carries the message_board SessionRef recipient address and MailSender::Session; only the LocalObservation phantom differs",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/remote_admission.rs",
    line: 47,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "send_remote flattens the target SessionRef's collaboration_protocol ServiceId to a String and rebuilds a message_board ServiceId with try_into for the installation peer lookup",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "service-identity-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/remote_calls.rs",
    line: 156,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "dispatch_remote re-types the resolved DeliveryEnvelope<RouterDeliveryTypes> as DeliveryEnvelope<BoardDeliveryTypes> by to_value (encode_answer) then from_value before mail_submit, carrying the SessionRef recipient",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/remote_wakes.rs",
    line: 24,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "convert<T>: generic serde_json to_value then from_value between any two types, mapping every failure to (); the conversion used for every remote-wake identity, request, result and failure mapping in this file (69-497)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/remote_wakes.rs",
    line: 94,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "send_wake_mail flattens the target SessionRef's collaboration_protocol ServiceId to a String and rebuilds a message_board ServiceId with try_into; LocalWakeOwner::apply compares the board recipient ServiceId with String::from(protocol UuidIdentity) (344-345)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "service-identity-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/remote_wakes.rs",
    line: 132,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "send_wake_mail converts the stored request target (collaboration_protocol SessionRef, automation_storage RemoteWakeMail) to the message_board SessionRef recipient address by convert",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/remote_wakes.rs",
    line: 371,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "LocalWakeOwner::apply compares the wake snapshot's collaboration_protocol message target with the board recipient address by convert::<message_board::SessionRef> (a failed conversion reads as a mismatch); the Send arm compares message.target with the recipient directly (390)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/remote_wakes.rs",
    line: 394,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "mail-protocol RemoteWakeRequest fields converted into the canonical MCP wake request types: WakeSendRequest {operationId, requestedBy, message (board SessionRef target), timing, expiry} (394-399), WakeShowRequest wakeupId (363, 410), WakeMutationRequest {operationId, wakeupId} (429-430), WakeWaitRequest wakeupId (450). The copied mail types are indexed separately in collaboration-mail-protocol (Lead 01a1e2cd).",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/remote_wakes.rs",
    line: 402,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "MCP wake results to mail-protocol RemoteWakeResult by convert: WakeSnapshot to RemoteWakeSnapshot (402, 407, and the stored replay snapshot 69), wake mutation result to RemoteWakeMutation (438), wait answer to RemoteWakeWaitResult (465)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_delivery_runtime/remote_wakes.rs",
    line: 493,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "remote_failure maps WakeFailureReason to RemoteWakeRefusal and LocalMutationState to RemoteWakeMutationState by JSON round trip, silently coercing an unmatched reason to InvalidRecord and state to Unknown (497); WakeWaitFailure::InvalidField to RemoteWakeRefusal::InvalidField by hand (467-475)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_push_outcome.rs",
    line: 40,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "observed_outcome maps Router DeliveryOutcome {Started, Steered, StartedOrSteered, Queued, PeerMessageWritten} to fleet LocalOutcome::Delivered with mail CarrierAcceptance, Unknown to Uncertain, NotSubmitted/Rejected to None (then classified by UnsentCause to DeliveryRefusal 22-31)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/mail_push_outcome.rs",
    line: 84,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "local_follow_outcome repeats the DeliveryOutcome to LocalOutcome/CarrierAcceptance arms of observed_outcome (40), but maps NotSubmitted{retryable} to Held and NotSubmitted/Rejected to a scheduled Held",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/native_claude_catalogue.rs",
    line: 213,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "peer_status maps claude_code_peer_messaging PeerSessionStatus {Busy, Idle, Waiting, Shell, Unreported, Other(_)} to the MCP ClaudeCodeInteractiveStatus, dropping Other's value; feeds ProviderSessionSummary rows of provider_sessions_list (151, 182)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/native_loaded_permission_fixture.rs",
    line: 164,
    owner: "codex",
    kind: "method-table",
    defines:
      "cfg(test) fake Codex: reply table for initialize, thread/read l166, thread/resume l169, turn/start l181, thread/turns/list l200, turn/steer l203, thread/queue/add l204; reply envelope {id,result} l216; also hand-built Router run JSON decoded by from_value: CapturedRunInputs l110, CodexAppServer recorded receipt l111",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "codex-stand-in-schemas",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: "crates/collaboration-service/src/native_wake_acceptance_fixture.rs",
    line: 30,
    owner: "codex",
    kind: "hand-json",
    defines:
      "cfg(test) fake Codex: initialize result l19, thread/read method check l30 and result {thread:{id,status:{type:idle}}} l35, turn/start method check l47, reads params.clientUserMessageId l51, turn/start result {turn:{id}} l57",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "codex-stand-in-schemas",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: "crates/collaboration-service/src/notification_publisher/handoff_scan.rs",
    line: 96,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "publish_delivery_handoff re-types the frozen DeliveryItem<BoardDeliveryTypes> as DeliveryItem<RouterDeliveryTypes> by to_value then from_value, carrying the SessionRef recipient",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/notification_publisher/local_cancellation.rs",
    line: 17,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "before_local_follow_injection converts collaboration_protocol SessionRef to message_board SessionRef with try_into and compares it with the frozen envelope's recipient address (57)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/notification_publisher/local_cancellation.rs",
    line: 199,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "persist_local_cancel re-types DeliveryCancel<BoardDeliveryTypes> as DeliveryCancel<RouterDeliveryTypes> by to_value then from_value, carrying the SessionRef recipient",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/notification_publisher/remote_cancellation.rs",
    line: 101,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "expand_remote_cancel_group re-types DeliveryCancel<BoardDeliveryTypes> as DeliveryCancel<RouterDeliveryTypes> by to_value then from_value, carrying the SessionRef recipient",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/operation_receipt_projection.rs",
    line: 13,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "stored method string decoded to AutomationOperationMethod through Value::String (13); success re-serializes method.as_str() and decodes it into each OperationSuccess family's own method enum (93; 97, 121, 135, 144, 152, 161), so the family method enums are JSON-coupled copies of AutomationOperationMethod",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "operation-method-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/operation_receipt_projection.rs",
    line: 50,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      'project_failure builds the MCP OperationFailure {kind, stage, message, field, constraint, nextAction, effects} from ScheduleFailure (58-71) and ConfigurationFailure (74-83), flattening typed kind/stage/nextAction to strings with token (169) and a literal stage "configuration" (77); OperationState::Uncertain reads failure.message from stored JSON by hand (19)',
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/project_admission/membership_mail_cancels.rs",
    line: 35,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "record_membership_cancel: a fresh UUIDv7 string parsed into MailItemRef (L35) and the board membership RecordId re-parsed by as_str().to_owned().try_into() into the MailCancelGroup membership id (L36-42)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/project_admission/mod.rs",
    line: 116,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "ProjectAdmission::new: collaboration_contracts MachineIdentity service_id to message_board ServiceId by String round trip (l116), repeating mail_delivery_runtime.rs:277 and remote_admission.rs:47; the separate machine_label to MachineLabel mapping is the l118 row",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "service-identity-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/project_admission/project_rejection.rs",
    line: 6,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "registration_rejection: fleet_sync_protocol RegistrationRefusal::DatabaseRejected{reason} to message_board ProjectRegistrationRejection {code: reason.code string, detail}; the identical mapping is message-board-storage project_ledger_reader.rs:59 rejection_reason; reaches MCP as ScopeRefusal::ProjectRejected{reason: Registration}",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "failure-kind-mapping",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/project_admission/read_mark_admission.rs",
    line: 122,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "gate_reason: RequestResult NotWriter, LeaseExpired, TransferInProgress, SchemaNotReady, Overloaded copied into ReadMarkNotAdvanced (message-board read_mark_outcomes.rs:35), a second enum for the same write-gate refusals carried in MCP ThreadShowResult.read_mark and BoardReadReply.read_mark; any other RequestResult becomes an InvalidRecord error",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/project_admission/reader_state.rs",
    line: 77,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "reader_snapshot: RecordRef subject of kind Thread re-parsed by id string into ThreadId (L77) to build ReadScope::Thread for the WriteReply.reader state",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/project_admission/thread_operations.rs",
    line: 443,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "post_record: MCP thread_create/thread_post references RecordRef {kind, id} re-parsed by id string into ReferenceTarget::Post{postId} (L445) or ReferenceTarget::Thread{threadId} (L452); any other kind is refused as InvalidShape references",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/project_admission/write_gate.rs",
    line: 25,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "gate_result: LeaseRefusal::NotRegistered, Released, NotWriter to RequestResult::NotWriter{current:None}; Superseded to NotWriter{current} (EpochNumber L59); SchemaNotReady and SchemaTooNew to SchemaNotReady; LeaseExpired to LeaseExpired{expiredAt} (millis to UTC L44); Draining to TransferInProgress (TransferId by string L52); Rejected, FileInUse, Closed, Storage, Provisional, Seeding, InvalidKey, KeyChange, StatementRefused to None. RequestResult is the MCP board write reply (WriteReply.result)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "failure-kind-mapping",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/provider_session_inventory.rs",
    line: 16,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "ProviderInventoryCursor {endpoint, view, scope, source, query, sortTimeMs, sessionId, origin: ProviderInventoryOrigin managed|nativePeer|nativeBackground (L108), serviceEpoch, expiresAt} serialized as base64url JSON inside the opaque MCP provider_sessions_list cursor string (decode L205-206, encode L384-386)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "provider-inventory-cursor",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/provider_session_inventory.rs",
    line: 37,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "view_name: hand string table stored|loaded|active repeating the serde names of NativeSessionView, stored in and compared against the cursor view field (L211, L374)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "provider-inventory-cursor",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/provider_session_inventory.rs",
    line: 45,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "state_tag: session_event_model SessionState to protocol ProviderSessionState (RequiresAction payload dropped); same mapping as codex-router-host provider_session_inspection.rs:131",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "session-state-mapping",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/provider_session_inventory.rs",
    line: 174,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "literal endpoint id claude-local compared with String::from(endpoint_id) decides the Claude Code native catalogue path and its source rules (L174-182, L236, L345)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/provider_session_inventory.rs",
    line: 322,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "protocol SessionRef (ProviderSessionRecord.target) to board SessionRef for SessionEventHub::state by serde_json to_value then from_value, by hand instead of the conversion at collaboration-protocol mail_boundary_conversions.rs:9",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/push_record_resolver.rs",
    line: 12,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "MachineId::from(ServiceId) to build RouterLink (link_for L12, line_for L24); resolve_reference compares the parsed link machine id string with String::from(service_id) (L42)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/push_record_resolver.rs",
    line: 22,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "line_for builds PushLineInput {link: RouterLink(MachineId, pushId), machineLabel, origin, headerFacts, body} for render_push_line; the same hand construction is repeated at direct_message_push.rs:104, subscription_push.rs:182 and reader_poll_mail.rs:193",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "push-line-construction",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/run_projection.rs",
    line: 21,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "agent_automation CapturedRunInputs<SessionRef, EndpointRef> to collaboration_protocol CapturedRunInputs (L21-22) and agent_automation WorkerOutcome to collaboration_protocol WorkerOutcome (L30-31) by serde_json to_value then from_value, for the MCP RunSnapshot",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/run_projection.rs",
    line: 82,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "ClaudeCodePeer route evidence session_id string compared with String::from(target.session_id) of the captured OwnedThread destination to recover the target SessionRef (L77-87)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/run_projection.rs",
    line: 116,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RunPhase to protocol RunState hand mapping (Waiting, Preparing, Executing, Stopping, SummaryRequired, SummaryRunning, SummaryBlocked, Finished, PreparationFailed, Uncertain) with literal explanation strings for SummaryBlocked (L123), PreparationFailed (L125) and Uncertain (L126)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/schedule_activation.rs",
    line: 31,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "agent_automation ExecutionDestination to collaboration_contracts ScheduleDestination: FreshEachRun to Fresh{endpoint}, OwnedThread to Existing{target}; FreshEachRunUnprepared and Unprepared refused with literal reasons (L31-74)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/schedule_activation.rs",
    line: 57,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "OwnedThread target SessionRef flattened to BindingAddress {service_id, endpoint_id, thread_id} strings by String::from (L57-62)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
      {
        crate: "automation-storage",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/schedule_preparation.rs",
    line: 42,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP schedule_prepare DestinationPreparation Existing/Fresh/Fork to collaboration_contracts ScheduleDestination Existing/Fresh/Fork with cwd dropped (L42-57)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
];
