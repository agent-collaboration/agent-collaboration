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
];
