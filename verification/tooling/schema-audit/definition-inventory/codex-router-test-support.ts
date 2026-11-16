import type { InventoryRow } from "../schema-audit-types.ts";

const R = "crates/codex-router-test-support/src/";

export const rows: InventoryRow[] = [
  {
    file: `${R}native_app_server.rs`,
    line: 50,
    owner: "codex",
    kind: "hand-json",
    defines:
      "stand-in Codex app-server (run_native_app_server_fixture): reads the initialize id (45-48) and the initialized method (60) by hand, replies initialize result {userAgent: codex-cli/<version>} (51-54) and remoteControl status result {status:connected,serverName,environmentId} (70-77)",
    usedBy: [{ part: "N10" }],
    duplicateGroup: "codex-stand-in-schemas",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: `${R}native_app_server.rs`,
    line: 102,
    owner: "codex",
    kind: "hand-json",
    defines:
      "stand-in Codex app-server (run_delayed_native_app_server_observation_fixture): initialize result {userAgent} (104-107), remoteControl status result {status:connecting,serverName,environmentId} (120-126) and a remoteControl/status/changed notification {method,params:{status:connected,serverName,environmentId}} (135-142)",
    usedBy: [{ part: "N10" }],
    duplicateGroup: "codex-stand-in-schemas",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: `${R}owned_peer_fixture.rs`,
    line: 84,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "fixture target built as the typed SessionRef from the caller's EndpointRef and SessionId, then its endpoint_id overwritten with the literal endpoint id claude-local (84-88); the session id is written into the registry record by String::from (217); typed construction, not a hand-written SessionRef shape: the defect is the repeated endpoint literal",
    usedBy: [{ crate: "codex-router-test-support" }],
    duplicateGroup: "endpoint-id-literals",
    disposition: "duplicate",
    fixedBy: "D2",
  },
];
