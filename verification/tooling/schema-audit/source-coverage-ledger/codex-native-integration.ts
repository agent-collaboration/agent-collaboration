import type { LedgerSection, SendSiteCoverage } from "../schema-audit-types.ts";

const R = "crates/codex-native-integration/src/";
const N10 = { part: "N10" } as const;
const TAP = "crates/codex-native-integration/src/validated_native_request_node_tap_test.rs";
const APPROVAL_TAP = "crates/codex-acp-adapter/src/native_prompt_execution_node_tap_test.rs";
/**
 * N11's native-control case drives the real NativeProtocolConnection::connect (through the
 * service's native_call, session_native_control.rs:302) against a fake Codex socket and records
 * every frame at WireFrame (mcp_server_node_tap_wire.rs:265-305). The initialize/initialized
 * handshake records are marked setup: checked, but not part of the routed row; the fake's
 * replies are synthetic apparatus.
 */
const N11_HANDSHAKE_TAP: SendSiteCoverage = {
  kind: "tap",
  testFile: "crates/collaboration-mcp/src/mcp_server_node_tap_test.rs",
  cases: ["native_control_tool_to_n10"],
};
const noTap = (reason: string) => ({ kind: "uncovered", reason }) as const;

export const section: LedgerSection = {
  root: "crates/codex-native-integration",
  files: [
    {
      file: `${R}app_server_launch.rs`,
      disposition: { kind: "none" },
      note: "command line of the managed app-server child ('app-server' argument :73)",
    },
    {
      file: `${R}codex_project_trust.rs`,
      disposition: { kind: "none" },
      note: "read-only projection of Codex config trust keys; no wire shape",
    },
    {
      file: `${R}debug_endpoint_selection.rs`,
      disposition: { kind: "none" },
      note: "debug socket selection policy",
    },
    {
      file: `${R}debug_profile_projection.rs`,
      disposition: { kind: "none" },
      note: "Codex config overrides for the debug profile; config, not a message",
    },
    {
      file: `${R}desktop_launch_policy.rs`,
      disposition: { kind: "none" },
      note: "macOS launch-session policy",
    },
    { file: `${R}lib.rs`, disposition: { kind: "none" }, note: "module list and re-exports" },
    {
      file: `${R}native_executable_identity.rs`,
      disposition: { kind: "none" },
      note: "executable path and content hash",
    },
    {
      file: `${R}native_input_submission.rs`,
      disposition: { kind: "converts", owners: ["codex"] },
      note: "turn/start and turn/steer params json! (:21-29, :40, :64-65) sent without schema validation",
    },
    {
      file: `${R}native_payload_schemas.rs`,
      disposition: { kind: "defines", owners: ["codex"] },
      note: "NativeOperation enum and the method/Params/Response name table for 13 operations (:6-76); ServerRequest and ServerNotification validators (:141); second hand sha256 hex digest string (:132-139)",
    },
    {
      file: `${R}native_protocol_connection.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "initialize (:100, written by request() at :149), initialized (:104), generic request envelope {id,method,params} (:141), callback response {id,result} (:51); hand-parses inbound frames: method present means buffered notification/request (:160), id match and result vs error.code (:170-184)",
    },
    {
      file: `${R}native_protocol_observation.rs`,
      disposition: { kind: "converts", owners: ["codex"] },
      note: "hand-built initialize json! on the control socket (:194); inbound InitializeResult Deserialize stand-in (:230) and userAgent version parser (:218); read_response matches id and takes result by hand (:142-148); sends it",
    },
    {
      file: `${R}native_schema_bundle.rs`,
      disposition: { kind: "defines", owners: ["codex"] },
      note: "canonical bundle {entrypoint, documents} json! (:62) of the exported Codex schemas",
    },
    {
      file: `${R}native_schema_digest.rs`,
      disposition: { kind: "defines", owners: ["codex"] },
      note: "NativeSchemaDigest (:9): hand sha256:<hex> decoder FromStr (:33-46) and hand hex encoder Display (:49-56)",
    },
    {
      file: `${R}native_schema_export.rs`,
      disposition: { kind: "reads", owners: ["codex"] },
      note: "runs 'app-server generate-json-schema --experimental --out' (:28-35) and collects the result",
    },
    {
      file: `${R}native_schema_validation.rs`,
      disposition: { kind: "converts", owners: ["codex"] },
      note: "wraps one v2 or root definition in a draft-07 $ref schema json! (:58-63)",
    },
    {
      file: `${R}native_session_launch.rs`,
      disposition: { kind: "none" },
      note: "direct-session launch arguments and working-directory flags",
    },
    { file: `${R}native_state_paths.rs`, disposition: { kind: "none" }, note: "Codex state paths" },
    {
      file: `${R}native_thread_operations.rs`,
      disposition: { kind: "converts", owners: ["codex"] },
      note: "thread/resume (:11), thread/read (:33), turn/interrupt (:56) params json!, sent without schema validation",
    },
    {
      file: `${R}remote_control_observation.rs`,
      disposition: { kind: "converts", owners: ["codex"] },
      note: "initialized and remoteControl/status/read json! (:52-58); inbound RemoteStatus and RemoteStatusKind Deserialize stand-ins (:105-143) decoded from the read result (:64) and remoteControl/status/changed params (:83-93); RemoteStatus to RemoteControlObservation mapping (:114); sends them",
    },
    {
      file: `${R}remote_resume_permission_keys.rs`,
      disposition: { kind: "none" },
      note: "list of Codex profile keys (default_permissions and others, :8-20); config, not wire",
    },
    {
      file: `${R}router_profile_projection.rs`,
      disposition: { kind: "none" },
      note: "Codex profile projection for the router; config",
    },
    {
      file: `${R}router_scratch_scopes.rs`,
      disposition: { kind: "none" },
      note: "scratch folder and lock files",
    },
    {
      file: `${R}router_tool_locations.rs`,
      disposition: { kind: "none" },
      note: "tool location lists",
    },
    {
      file: `${R}schema_bundle_storage.rs`,
      disposition: { kind: "converts", owners: ["codex"] },
      note: "immutable schema byte storage; bundle file name is a third hand hex encoding of the schema digest (bundle_file_name :74-81)",
    },
    {
      file: `${R}schema_export_collection.rs`,
      disposition: { kind: "none" },
      note: "bounded filesystem collection of the export directory",
    },
    {
      file: `${R}schema_json_validation.rs`,
      disposition: { kind: "reads", owners: ["codex"] },
      note: "SchemaValue custom Deserialize (:3-14) that decodes exported schema documents strictly",
    },
    {
      file: `${R}scratch_scope_test_support.rs`,
      disposition: { kind: "none" },
      note: 'test-only barrier; declared under #[cfg(feature = "test-support")] (lib.rs:93-94), not cfg(test); no message shape',
    },
    {
      file: `${R}session_search_expression.rs`,
      disposition: { kind: "none" },
      note: "session search semantics",
    },
    {
      file: `${R}stored_repository_identity.rs`,
      disposition: { kind: "none" },
      note: "repository membership from the stored Codex thread row",
    },
    {
      file: `${R}stored_thread_catalog.rs`,
      disposition: { kind: "none" },
      note: "read-only open of the Codex state database",
    },
    {
      file: `${R}stored_thread_query.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "SQLite query over Codex's stored thread table; append_source_filter (:318-333) maps the Router StoredThreadSource selection onto Codex source/thread_source values, a copy of the session-source mapping (inventory row :324)",
    },
    {
      file: `${R}validated_native_request.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "request_validated: validates params and result against the bundle around request() (:20)",
    },
  ],
  sendSites: [
    {
      file: `${R}native_protocol_connection.rs`,
      line: 100,
      edge: "managed-codex",
      sends:
        "initialize (clientInfo agent_collaboration, experimentalApi); recorded as a setup frame by N11's native_control_tool_to_n10, not by N10's own tap (which connects through from_websocket)",
      owner: N10,
      origin: "product",
      coverage: N11_HANDSHAKE_TAP,
    },
    {
      file: `${R}native_protocol_connection.rs`,
      line: 104,
      edge: "managed-codex",
      sends:
        "initialized notification; recorded as a setup frame by N11's native_control_tool_to_n10, not by N10's own tap (which connects through from_websocket)",
      owner: N10,
      origin: "product",
      coverage: N11_HANDSHAKE_TAP,
    },
    {
      file: `${R}native_protocol_connection.rs`,
      line: 149,
      edge: "managed-codex",
      sends: "request envelope {id, method, params}",
      owner: N10,
      origin: "product",
      coverage: {
        kind: "tap",
        testFile: TAP,
        cases: [
          "node_tap_valid_reply_returns_response_and_keeps_connection",
          "node_tap_malformed_reply_to_effectful_call_is_outcome_unknown_and_retires",
          "node_tap_malformed_reply_to_read_is_protocol_and_retires",
          "node_tap_error_reply_is_rejected_and_keeps_connection",
        ],
      },
    },
    {
      file: `${R}native_protocol_connection.rs`,
      line: 58,
      edge: "managed-codex",
      sends: "callback response {id, result} to a server request (approval)",
      owner: N10,
      origin: "product",
      coverage: {
        kind: "tap",
        testFile: APPROVAL_TAP,
        cases: ["node_tap_legacy_codex_approval_goes_to_broker_and_cancel_returns_to_codex"],
      },
    },
    {
      file: `${R}validated_native_request.rs`,
      line: 20,
      edge: "managed-codex",
      sends: "any typed operation after both-side schema validation",
      owner: N10,
      origin: "product",
      coverage: {
        kind: "tap",
        testFile: TAP,
        cases: [
          "node_tap_valid_reply_returns_response_and_keeps_connection",
          "node_tap_malformed_reply_to_effectful_call_is_outcome_unknown_and_retires",
          "node_tap_malformed_reply_to_read_is_protocol_and_retires",
          "node_tap_error_reply_is_rejected_and_keeps_connection",
        ],
      },
    },
    {
      file: `${R}native_input_submission.rs`,
      line: 40,
      edge: "managed-codex",
      sends: "turn/start without schema validation",
      owner: N10,
      origin: "product",
      coverage: noTap("the tap sends turn/start through request_validated, not this path"),
    },
    {
      file: `${R}native_input_submission.rs`,
      line: 65,
      edge: "managed-codex",
      sends: "turn/steer without schema validation",
      owner: N10,
      origin: "product",
      coverage: noTap("no tap records turn/steer"),
    },
    {
      file: `${R}native_thread_operations.rs`,
      line: 11,
      edge: "managed-codex",
      sends: "thread/resume without schema validation",
      owner: N10,
      origin: "product",
      coverage: noTap("no tap records this path"),
    },
    {
      file: `${R}native_thread_operations.rs`,
      line: 34,
      edge: "managed-codex",
      sends: "thread/read without schema validation",
      owner: N10,
      origin: "product",
      coverage: noTap("the tap sends thread/read through request_validated, not this path"),
    },
    {
      file: `${R}native_thread_operations.rs`,
      line: 56,
      edge: "managed-codex",
      sends: "turn/interrupt without schema validation",
      owner: N10,
      origin: "product",
      coverage: noTap(
        "no tap drives this helper; N11's native_control_tool_to_n10 records turn/interrupt sent through the service's native_call and request_validated (session_native_control.rs:106-122, :312), a different call site",
      ),
    },
    {
      file: `${R}native_protocol_observation.rs`,
      line: 194,
      edge: "managed-codex",
      sends: "initialize on the control socket (clientInfo codex_router_host)",
      owner: N10,
      origin: "product",
      coverage: noTap("no tap runs the control observation"),
    },
    {
      file: `${R}remote_control_observation.rs`,
      line: 52,
      edge: "managed-codex",
      sends: "initialized notification on the control socket",
      owner: N10,
      origin: "product",
      coverage: noTap("no tap runs the remote control observation"),
    },
    {
      file: `${R}remote_control_observation.rs`,
      line: 55,
      edge: "managed-codex",
      sends: "remoteControl/status/read",
      owner: N10,
      origin: "product",
      coverage: noTap("no tap runs the remote control observation"),
    },
  ],
};
