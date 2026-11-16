import type { LedgerSection, SendSite } from "../schema-audit-types.ts";
// Left out by name only: 4 colocated unit-test files (*_test.rs, *_tests.rs).
// Test support: every send in this crate goes from a fake peer (stand-in OpenAI upstream, stand-in
// Codex CLI client, stand-in Codex app-server, stand-in Claude Code peer, fake Router healthz) to
// the Router under test, or from the Router under test to that fake; none is a product send. The
// stand-in Codex app-server's replies (native_app_server.rs) speak a scoped protocol, so they are
// listed as apparatus send sites; the OpenAI Responses fixtures, the fake healthz and the Claude
// Code peer fixture's peer frames speak none of Project 2's protocols: they have no send rows and
// no inventory rows, and the Responses and healthz files are excluded. Files under src/node_taps/
// added after the base commit (connection_ledger, edge_conformance*, json_path, live_recording, recording,
// schema_settlement) are not base files and so have no row here.

const NATIVE = "crates/codex-router-test-support/src/native_app_server.rs";
const noKeptCapture: SendSite["coverage"] = {
  kind: "uncovered",
  reason:
    "test apparatus: the fixture answers on its own accepted WebSocket; no kept complete capture records it (the N10 tap validated_native_request_node_tap_test.rs builds and records its own socket pair)",
};

/** A stand-in Codex app-server write over the fixture's accepted native WebSocket. */
function standInNativeSend(line: number, sends: string): SendSite {
  return {
    file: NATIVE,
    line,
    edge: "managed-codex",
    sends,
    owner: { crate: "codex-router-test-support" },
    origin: "apparatus",
    coverage: noKeptCapture,
  };
}

const fakePeerSends =
  "its sends go to or from the Router under test as a fake peer, not product sends on one of the six outside edges";

export const section: LedgerSection = {
  root: "crates/codex-router-test-support",
  files: [
    {
      file: "crates/codex-router-test-support/src/global_claude_fixture.rs",
      disposition: { kind: "none" },
      note: "Writes a fixture `claude` shell script (L13) and prepends its directory to PATH (L19-L35); no message shape.",
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex.rs",
      disposition: { kind: "none" },
      note: "Module root of the installed-Codex smoke harness: re-exports, imports and constants (smoke prompt, model, timeouts L84-L106); no message shape.",
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/concurrent_websocket.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic stand-ins)",
      },
      note: `Stand-in OpenAI Responses WebSocket upstream (Responses model traffic the proxy relays on the N2 cli-codex edge, not app-server): hand-built response.created / response.output_item.done function_call / response.completed frames with json! L495-L541; hand readers of the response.create frame's model (L222-L233), the harness client marker (L211) and function_call_output in the follow-up input (L605-L615); ${fakePeerSends}.`,
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/floor_switch.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at installed_codex.rs:4 under #[cfg(test)] (installed_codex.rs:3)",
      },
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/floor_switch/upstream.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in installed_codex/floor_switch.rs:3, itself a test module (declared at installed_codex.rs:4 under #[cfg(test)])",
      },
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/hostile_and_state.rs",
      disposition: { kind: "none" },
      note: "Seeds smoke accounts, quota windows and credentials into the Router state store (L146-L405) and runs `codex-router quota status` in-process (L422-L461). Carries one hand reader, selected_account_from_status_json (L469-L499: accounts[].preferred_next, safe_account_label, account_hash), of the CLI quota status JSON report: local CLI output, which the codex-router-quota-service ledger (quota_status_json.rs) treats as outside the four protocols. The hostile no-token frame it triggers is built in http_probe.rs:676.",
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/http_probe.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic stand-ins)",
      },
      note: `Stand-in OpenAI Responses upstream and stand-in Codex CLI client (Responses traffic on the N2 cli-codex edge): SSE body framing event:/data: (L329-L340) over hand-built SSE events (json! L347-L415); WebSocket prewarm and response events (json! L551-L607); hand readers of response.create frames (is_prewarm_request_frame L529, is_non_prewarm_response_create_frame L535-L546); client frames {type:response.create,hostile_no_token} (L676) and a response.create request payload (json! L692-L703); ${fakePeerSends}.`,
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/process_runtime.rs",
      disposition: { kind: "none" },
      note: "Spawns the codex-router serve child and the installed `codex exec` child, reads readiness lines and child output (L9-L533); no message shape.",
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/quota_reconnect_upstream.rs",
      disposition: { kind: "none" },
      note: "Accept loop of the concurrent stand-in upstream (L8-L89); dispatches to http_probe.rs and concurrent_websocket.rs, builds no frame.",
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/retry.rs",
      disposition: { kind: "none" },
      note: `Retry-scenario stand-in upstream: seeds accounts (L135-L208), edits the Codex profile TOML (L119-L133) and sends frames built elsewhere (smoke_prewarm_events / smoke_response_events from http_probe.rs, quota_reconnect_usage_limit_frame from smoke_contracts.rs) at L356-L385; it defines no frame of its own; ${fakePeerSends}.`,
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/s8_provenance.rs",
      disposition: { kind: "none" },
      note: "S8 smoke report JSON and redaction checks (json! L10, L34, L65, L73, L380, L393): the harness's own artifact, not a protocol message; reuses http_probe.rs's response.create reader.",
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/smoke_contracts.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic stand-ins)",
      },
      note: "Stand-in upstream Responses error frame {type:error,status:429,error:{type:usage_limit_reached,code:usage_limit_reached}} as a raw string (L293-L295); hand reader of the HTTP/SSE request body's stream flag (L151-L156) and body keys (L158-L184); the rest asserts on captured headers and tokens.",
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/smoke_modes.rs",
      disposition: { kind: "none" },
      note: "Smoke mode configuration and journeys (L20-L529); UpstreamClientSessionObservation (L190) is a Serialize field of the harness's own report; no protocol shape.",
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/smoke_runtime.rs",
      disposition: { kind: "none" },
      note: "Three-client WebSocket journey runner, runtime-root policy and contract assertions over the transcript structs (L17-L576); no message shape.",
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at installed_codex.rs:136 under #[cfg(test)] (installed_codex.rs:134)",
      },
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/transcript_observation.rs",
      disposition: { kind: "none" },
      note: "Carries serde shapes outside the four protocols: RouterWebSocketRegistryReport and RouterWebSocketRegistryReportFile Deserialize (L33, L54), the proxy's --websocket-registry-report-file diagnostic file; a hand reader of the Router audit JSONL (local_auth_result, outcome, transport_kind L105-L116); and the redacted smoke report JSON (json! L196, L310, L353, L451, L638). Router diagnostics and harness artifacts, not messages on the six outside edges.",
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/websocket_models.rs",
      disposition: { kind: "none" },
      note: "Transcript and state structs of the stand-in upstreams and the Router process guard (L10-L812); first_frame_json (L28) only parses the captured frame to a Value; no shape defined.",
    },
    {
      file: "crates/codex-router-test-support/src/installed_codex/websocket_upstream.rs",
      disposition: { kind: "none" },
      note: `Stand-in upstream accept loops and the SQLite pressure helper launcher (L13-L455); sends frames built in http_probe.rs and smoke_contracts.rs (L204-L258) and defines none; ${fakePeerSends}.`,
    },
    {
      file: "crates/codex-router-test-support/src/lib.rs",
      disposition: { kind: "none" },
      note: "Module list and FixtureResult alias.",
    },
    {
      file: "crates/codex-router-test-support/src/mock_upstream.rs",
      disposition: { kind: "none" },
      note: "In-memory list of TranscriptRequest values (L7-L22); bodies kept as bytes, not interpreted.",
    },
    {
      file: "crates/codex-router-test-support/src/native_app_server.rs",
      disposition: { kind: "converts", owners: ["codex"] },
      note: `Stand-in Codex app-server (managed-codex peer for shared-host proofs): reads the initialize id and the initialized method by hand (L45-L48, L60, L97-L100) and answers with hand-built initialize result {userAgent} and remoteControl status result {status,serverName,environmentId} (json! L51-L54, L70-L77, L104-L107, L120-L126) plus a remoteControl/status/changed notification (json! L135-L142); its five WebSocket writes (L50, L70, L103, L119, L134) are apparatus send sites, the Host under test (codex-router-host and codex-router-cli integration tests) is the counterparty.`,
    },
    {
      file: "crates/codex-router-test-support/src/node_taps.rs",
      disposition: {
        kind: "excluded",
        reason:
          "D0 node-tap apparatus, not a protocol participant: its serde shapes (ProtocolNode L49, EdgeParty L97, MessageFamily L118, TapRecord L149, TapVerdict L278, TapOutcome L286, TapCase L314, TapCaseResult L340) are the tap recording file format read by verification/tooling, and protocol messages are kept as opaque serde_json::Value in TapRecord.raw/payload (L169, L173); no message on the six outside edges is defined or converted here",
      },
      note: "Changed after the base commit (D0 apparatus); lines are working-tree lines.",
    },
    {
      file: "crates/codex-router-test-support/src/node_taps/expected_failures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "D0 node-tap apparatus: Deliverable, BehaviorDefect, SchemaDefect and RecordKey (L28, L51, L59, L74) serialize the expected-failure list into the tap recording file format; no protocol message shape",
      },
    },
    {
      file: "crates/codex-router-test-support/src/node_taps/expected_failures/n1.rs",
      disposition: { kind: "none" },
      note: "Constant expected-failure entries (case ids, defect text, schema paths); no message shape.",
    },
    {
      file: "crates/codex-router-test-support/src/node_taps/expected_failures/n10.rs",
      disposition: { kind: "none" },
      note: "Empty expected-failure list.",
    },
    {
      file: "crates/codex-router-test-support/src/node_taps/expected_failures/n11.rs",
      disposition: { kind: "none" },
      note: "Constant expected-failure entry; no message shape.",
    },
    {
      file: "crates/codex-router-test-support/src/node_taps/expected_failures/n12.rs",
      disposition: { kind: "none" },
      note: "Empty expected-failure list.",
    },
    {
      file: "crates/codex-router-test-support/src/node_taps/expected_failures/n3.rs",
      disposition: { kind: "none" },
      note: "Empty expected-failure list.",
    },
    {
      file: "crates/codex-router-test-support/src/node_taps/expected_failures/n4.rs",
      disposition: { kind: "none" },
      note: "Empty expected-failure list.",
    },
    {
      file: "crates/codex-router-test-support/src/node_taps/expected_failures/n5.rs",
      disposition: { kind: "none" },
      note: "Constant expected-failure entries; no message shape.",
    },
    {
      file: "crates/codex-router-test-support/src/node_taps/expected_failures/n6.rs",
      disposition: { kind: "none" },
      note: "Constant expected-failure entries; no message shape.",
    },
    {
      file: "crates/codex-router-test-support/src/node_taps/expected_failures/n7.rs",
      disposition: { kind: "none" },
      note: "Empty expected-failure list.",
    },
    {
      file: "crates/codex-router-test-support/src/node_taps/expected_failures/n8.rs",
      disposition: { kind: "none" },
      note: "Empty expected-failure list.",
    },
    {
      file: "crates/codex-router-test-support/src/node_taps/expected_failures/n9.rs",
      disposition: { kind: "none" },
      note: "Constant expected-failure entry; no message shape.",
    },
    {
      file: "crates/codex-router-test-support/src/owned_cli_process.rs",
      disposition: {
        kind: "excluded",
        reason: "cfg(test)-only module: declared at lib.rs:9 under #[cfg(test)] (lib.rs:8)",
      },
    },
    {
      file: "crates/codex-router-test-support/src/owned_peer_fixture.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: `Stand-in Claude Code peer: builds the target SessionRef (Router vocabulary) with endpointId claude-local (L84-L88). Its registry record (L213-L238), key file (L247) and peer socket auth and user frames (L299-L331) are Claude Code's direct interfaces, outside Project 2's protocols, and have no inventory rows; fixtureBarrier frames (L132, L300) are fixture-internal; ${fakePeerSends}.`,
    },
    {
      file: "crates/codex-router-test-support/src/route_native.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic stand-ins)",
      },
      note: `Route-native proof as a stand-in Codex CLI client: hand-built HTTP bodies for /v1/responses, /v1/memories/trace_summarize and /v1/responses/compact (L42, L53, L61); checks the WebSocket reply text for response.completed (L67); Responses traffic on the N2 cli-codex edge; ${fakePeerSends}.`,
    },
    {
      file: "crates/codex-router-test-support/src/route_native/client.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic stand-ins)",
      },
      note: `Stand-in Codex CLI client: raw HTTP request writer (L25-L32) and a hand-built response.create WebSocket frame as a raw string (L58); ${fakePeerSends}.`,
    },
    {
      file: "crates/codex-router-test-support/src/route_native/fixture.rs",
      disposition: { kind: "none" },
      note: "Seeds route-native accounts, quota windows and credentials and starts the in-process loopback Router (L32-L259); no message shape.",
    },
    {
      file: "crates/codex-router-test-support/src/route_native/upstream.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic stand-ins)",
      },
      note: `Stand-in OpenAI upstream: hand-built replies per path (SSE response.completed, models list, memories summary, compact id; L429-L437) and a WebSocket {type:response.completed} frame (L424); checks the relayed first frame by substring for type response.create, model and canary (L209-L215); ${fakePeerSends}.`,
    },
    {
      file: "crates/codex-router-test-support/src/router_health.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Router GET /healthz)",
      },
      note: "Fake Router GET /healthz for Host compatibility probes: serializes the typed RouterCompatibility from codex-router-core (L131-L142), the HTTP body between Router and Host that the core ledger places outside the four protocols; no hand shape, and not one of the six outside edges.",
    },
    {
      file: "crates/codex-router-test-support/src/scenario_finalization.rs",
      disposition: {
        kind: "excluded",
        reason: "cfg(test)-only module: declared at lib.rs:14 under #[cfg(test)] (lib.rs:13)",
      },
    },
    {
      file: "crates/codex-router-test-support/src/scenario_finalization_tests/cleanup_errors.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at scenario_finalization_tests.rs:340, inside the test module that scenario_finalization.rs:3-4 includes, itself declared at lib.rs:14 under #[cfg(test)]",
      },
    },
    {
      file: "crates/codex-router-test-support/src/signal_recording.rs",
      disposition: { kind: "none" },
      note: "Signal-recording child fixture: writes ready/sigterm lines to an event file (L17-L41); no message shape.",
    },
    {
      file: "crates/codex-router-test-support/src/transcript.rs",
      disposition: { kind: "none" },
      note: "TranscriptRequest {method,path,body bytes} (L5-L27), an uninterpreted capture; no message shape.",
    },
  ],
  sendSites: [
    standInNativeSend(
      50,
      "stand-in initialize result {id,result:{userAgent:codex-cli/<version>}} (run_native_app_server_fixture)",
    ),
    standInNativeSend(
      70,
      "stand-in remoteControl/status/read result {status:connected,serverName,environmentId} (run_native_app_server_fixture)",
    ),
    standInNativeSend(
      103,
      "stand-in initialize result {id,result:{userAgent}} after a delay (run_delayed_native_app_server_observation_fixture)",
    ),
    standInNativeSend(
      119,
      "stand-in remoteControl/status/read result {status:connecting,serverName,environmentId} (run_delayed_native_app_server_observation_fixture)",
    ),
    standInNativeSend(
      134,
      "stand-in remoteControl/status/changed notification {status:connected,serverName,environmentId} (run_delayed_native_app_server_observation_fixture)",
    ),
  ],
};
