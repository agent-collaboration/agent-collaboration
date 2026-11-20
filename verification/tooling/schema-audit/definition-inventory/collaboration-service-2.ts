import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
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
];
