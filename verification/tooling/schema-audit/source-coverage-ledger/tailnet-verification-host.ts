import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/tailnet-verification-host/src/";
const noTap =
  "the Tailnet verification driver sends this itself (N10 or N11 is only the counterparty); no kept capture records this driver's connection: the N10 tap (validated_native_request_node_tap_test.rs:90-133) pairs its own fake socket and the N11 tap (mcp_server_node_tap_wire.rs:34-138) records its own fixture caller";

export const section: LedgerSection = {
  root: "crates/tailnet-verification-host",
  files: [
    {
      file: `${R}bounded_command.rs`,
      disposition: { kind: "none" },
      note: "subprocess spawn, bounded output capture, SIGTERM/SIGINT flag and child wait only; no serde shape",
    },
    {
      file: `${R}d1_central_runtime.rs`,
      disposition: { kind: "none" },
      note: "central admin API, not one of the six edges: POST /admin/v1/machines/enroll with json {kind:'enrollMachine',value:EnrollMachine} l145, reads EnrollmentDecision l149; driver-local D1CentralPlan l24, CentralCommand l37 (stdin control), D1MachineFixture l51 (fixture file), status JSON l195/l240/l256",
    },
    {
      file: `${R}d1_host_lifecycle.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "speaks Codex app-server stdio directly: hand-built initialize l221, initialized l232, account/read l235 (no jsonrpc field); decodes inbound frames as NativeFrame l88 via parse_answer l95, account/read reply as AccountReadiness l142 (l147), reads initialize codexHome l227; native config.toml text l41 and native-ready.json l257 are driver evidence",
    },
    {
      file: `${R}d1_host_plan.rs`,
      disposition: { kind: "none" },
      note: "driver plan and allocation guards only: D1NativeArtifact l19, D1AuthPolicy l27, D1NativeReadinessPlan l35, private file reader l155",
    },
    {
      file: `${R}d1_native_runtime.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "NativeProtocolConnection::connect l127 (initialize + initialized inside codex-native-integration), request account/read json! l130, hand reads requiresOpenaiAuth/account l133; driver-local D1HostPlan l38 and status JSON l214/l239; session_inputs l103 builds central BoardSessionInputs from the fixture (central fleet link, not one of the six edges)",
    },
    {
      file: `${R}driver_error.rs`,
      disposition: { kind: "none" },
      note: "DriverError enum and category names only",
    },
    {
      file: `${R}driver_records.rs`,
      disposition: { kind: "none" },
      note: "generic evidence read/save (read_record l11, save_record l15) and sha256; no owned message shape",
    },
    {
      file: `${R}host_lifecycle.rs`,
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "calls MCP tool(control.sock, 'endpoints_list') l175 (tools/call via mcp_observation::http_post); hand reads service.json fields l147-163 (Tailnet service.json, outside Project 2's protocols, no inventory row); triggers observe_http_boundary l189 and observe_local_inventory l191; settings.jsonc l341 is private Router settings storage",
    },
    {
      file: `${R}host_plan.rs`,
      disposition: { kind: "none" },
      note: "driver HostPlan l66 and HttpsEndpoint l11 URL validation only",
    },
    {
      file: `${R}host_qualification.rs`,
      disposition: { kind: "none" },
      note: "driver qualification artifacts (QualifiedArtifact l22, Qualification l30), codesign checks and Tailscale Serve status JSON l116-237; not one of the four protocols",
    },
    {
      file: `${R}json_equality.rs`,
      disposition: { kind: "none" },
      note: "generic JSON number-tolerant equality same_json l26; inline cfg(test) module l52",
    },
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "module declarations only",
    },
    {
      file: `${R}main.rs`,
      disposition: { kind: "none" },
      note: "driver CLI dispatch only",
    },
    {
      file: `${R}mcp_observation.rs`,
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "http_post l118 transport; tool_payload l153 hand-built tools/call; tools/list l396; rpc_result l131 and tool_result l156 parse the response by hand; require_service l173 and Starting* stand-ins l243-273 decode the endpoints_list EndpointInventory; McpSelector l62 reads the service.json mcp selector (Tailnet service.json, outside Project 2's protocols, no inventory row)",
    },
    {
      file: `${R}owned_host.rs`,
      disposition: { kind: "none" },
      note: "process cleanup evidence types CleanupInventory l15, ProcessSignal l61, CleanupFailure l66, CleanupResult l78; not one of the four protocols",
    },
    {
      file: `${R}process_identity.rs`,
      disposition: { kind: "none" },
      note: "ps-table ProcessIdentity l10 parsing only",
    },
    {
      file: `${R}profile_projection.rs`,
      disposition: { kind: "none" },
      note: "byte-level debug Codex profile endpoint projection; no serde shape",
    },
  ],
  sendSites: [
    {
      file: `${R}mcp_observation.rs`,
      line: 118,
      edge: "mcp",
      sends:
        "transport: every tools/call and tools/list POST of this driver, over HTTP loopback or the control.sock unix socket",
      owner: { crate: "tailnet-verification-host" },
      origin: "apparatus",
      coverage: {
        kind: "uncovered",
        reason: noTap,
      },
    },
    {
      file: `${R}mcp_observation.rs`,
      line: 331,
      edge: "mcp",
      sends:
        "tools/call endpoints_list over loopback TCP with the tailnet Host authority (observe_local_inventory)",
      owner: { crate: "tailnet-verification-host" },
      origin: "apparatus",
      coverage: { kind: "uncovered", reason: noTap },
    },
    {
      file: `${R}mcp_observation.rs`,
      line: 394,
      edge: "mcp",
      sends: "tools/list, once per Host/Origin probe (nine probes, observe_http_boundary)",
      owner: { crate: "tailnet-verification-host" },
      origin: "apparatus",
      coverage: { kind: "uncovered", reason: noTap },
    },
    {
      file: `${R}host_lifecycle.rs`,
      line: 175,
      edge: "mcp",
      sends:
        "tools/call endpoints_list over control.sock (readiness loop, via mcp_observation::tool)",
      owner: { crate: "tailnet-verification-host" },
      origin: "apparatus",
      coverage: { kind: "uncovered", reason: noTap },
    },
    {
      file: `${R}d1_host_lifecycle.rs`,
      line: 221,
      edge: "managed-codex",
      sends: "initialize (stdio, direct to a native Codex app-server)",
      owner: { crate: "tailnet-verification-host" },
      origin: "apparatus",
      coverage: { kind: "uncovered", reason: noTap },
    },
    {
      file: `${R}d1_host_lifecycle.rs`,
      line: 232,
      edge: "managed-codex",
      sends: "initialized notification",
      owner: { crate: "tailnet-verification-host" },
      origin: "apparatus",
      coverage: { kind: "uncovered", reason: noTap },
    },
    {
      file: `${R}d1_host_lifecycle.rs`,
      line: 235,
      edge: "managed-codex",
      sends: "account/read {refreshToken:false}",
      owner: { crate: "tailnet-verification-host" },
      origin: "apparatus",
      coverage: { kind: "uncovered", reason: noTap },
    },
    {
      file: `${R}d1_native_runtime.rs`,
      line: 127,
      edge: "managed-codex",
      sends:
        "initialize and initialized, sent inside NativeProtocolConnection::connect for this caller",
      owner: { crate: "tailnet-verification-host" },
      origin: "apparatus",
      coverage: {
        kind: "uncovered",
        reason: noTap,
      },
    },
    {
      file: `${R}d1_native_runtime.rs`,
      line: 130,
      edge: "managed-codex",
      sends: "account/read {refreshToken:false} via NativeProtocolConnection::request",
      owner: { crate: "tailnet-verification-host" },
      origin: "apparatus",
      coverage: { kind: "uncovered", reason: noTap },
    },
  ],
};
