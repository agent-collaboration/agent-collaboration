import { globSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { runProcess, type CommandRunner } from "./process-command.ts";
import { decodeEvidenceJson } from "../evidence-json.ts";
import { display } from "./proof-artifact-values.ts";

function read(path: string): string {
  return readFileSync(path, "utf8");
}
function requireMarkers(input: {
  readonly label: string;
  readonly source: string;
  readonly markers: readonly string[];
}): string[] {
  return input.markers
    .filter((marker) => !input.source.includes(marker))
    .map((marker) => `${input.label} missing required marker: ${marker}`);
}
export function httpReleaseErrors(): string[] {
  const source = read("crates/codex-router-proxy/src/server.rs");
  const start = source.indexOf("struct AsyncProxyCredentialResolver");
  const end = source.indexOf("impl HttpAffinitySecretProvider");
  const errors = start === -1 || end === -1 ? ["missing AsyncProxyCredentialResolver section"] : [];
  const resolver = start === -1 || end === -1 ? "" : source.slice(start, end);
  for (const marker of [
    "prepare_streaming_http_request_async",
    "AsyncSqliteStateStore::open",
    "AsyncRepositoryBackedAccountSelector::new_with_runtime",
    "AsyncProxyCredentialResolver::new(\n            state_store.clone(),\n            self.secret_store.clone(),",
    "service.prepare_streaming_request_async(request).await",
    "state_store: AsyncSqliteStateStore",
    "secret_store: ProxyRuntimeSecretStore",
    ".load_account(account_id)\n            .await",
    ".activate_account_credential_generation_if_current_and_invalidate_quota(",
  ])
    if (!source.includes(marker))
      errors.push(`missing async HTTP/SSE release-path marker: ${marker}`);
  for (const marker of [
    "ProxyCredentialResolver::open",
    "SqliteStateStore::open",
    "state_database_path:",
    "secret_store_root:",
  ])
    if (resolver.includes(marker))
      errors.push(`async credential resolver still has sync/path marker: ${marker}`);
  for (const marker of [
    "prepare_context.prepare_streaming_http_request",
    "fn prepare_streaming_http_request(",
    "let prepared = tokio::task::spawn_blocking(move ||",
    "ProxyCredentialResolver::open(",
  ])
    if (source.includes(marker))
      errors.push(`sync HTTP/SSE request-time marker still present: ${marker}`);
  return errors;
}
const releaseFiles = [
  "crates/codex-router-proxy/src/server.rs",
  "crates/codex-router-proxy/src/websocket.rs",
  "crates/codex-router-proxy/src/upstream.rs",
  "crates/codex-router-proxy/src/http_sse.rs",
];
function resolverErrors(runner: CommandRunner): string[] {
  const result = runner(
    [
      process.execPath,
      join(process.cwd(), "verification/tooling/check-release-runtime-guardrails.ts"),
      "--print-release-source-paths",
    ],
    {},
  );
  const errors: string[] = [];
  let paths: string[] = [];
  if (result.status !== 0)
    errors.push(`release source path resolver failed: ${result.stderr.trim()}`);
  else {
    try {
      paths = z.array(z.string()).parse(decodeEvidenceJson(result.stdout));
    } catch (error) {
      errors.push(`release source path resolver did not emit JSON: ${String(error)}`);
    }
  }
  const missing = releaseFiles.filter((path) => !paths.includes(path)).sort();
  if (missing.length !== 0)
    errors.push(`release source path resolver missed required files: ${display(missing)}`);
  const forbidden = paths.filter((path) => path.startsWith("crates/codex-router-test-support/"));
  if (forbidden.length !== 0)
    errors.push(
      `release source path resolver included test-support files: ${display(forbidden.slice(0, 5))}`,
    );
  return errors;
}
const inventoryByRow: Readonly<Record<string, readonly string[]>> = {
  "G-13": [
    "runtime_shutdown_cancels_pending_first_frame_routing",
    "runtime_shutdown_cancels_pending_upstream_connect",
    "runtime_shutdown_cancels_active_duplex_pumps",
  ],
  "G-14": ["async_websocket_tunnel_forwards_first_frame_and_second_local_frame"],
  "G-15": ["runtime_shutdown_cancels_active_duplex_pumps"],
  "G-16": [
    "loopback_router_runtime_accepts_http_while_websocket_is_blocked",
    "served_router_http_uses_persisted_quota_while_background_refresh_is_blocked",
    "served_router_websocket_uses_persisted_quota_while_background_refresh_is_blocked",
  ],
  "G-19": ["served_router_websocket_uses_persisted_quota_while_background_refresh_is_blocked"],
  "G-20": ["async_websocket_tunnel_does_not_gate_forwarding_on_slow_affinity_recorder"],
  "G-17": [
    "three_codex_websocket_concurrent_e2e_shares_router_pid_and_overlaps",
    'run_test_filter "three_codex_websocket_concurrent_e2e_"',
  ],
  "G-18": [
    "three_codex_websocket_soak_holds_overlap_and_records_activity",
    'run_three_websocket_soak_filter "three_codex_websocket_soak_"',
    "PIPESTATUS[0]",
  ],
};
function inventoryErrors(row: string, runner: CommandRunner): string[] {
  const errors: string[] = [];
  const testList = (crate: string): string => {
    const result = runner(["cargo", "test", "-p", crate, "--", "--list"], {});
    if (result.status !== 0)
      errors.push(`cargo test -p ${crate} -- --list failed: ${result.stderr.trim()}`);
    return result.stdout;
  };
  const harness = row === "G-17" || row === "G-18";
  const source = harness
    ? `${testList("codex-router-test-support")}\n${read("tests/smoke/installed_codex_mock.sh")}`
    : `${testList("codex-router-proxy")}\n${row === "G-16" || row === "G-19" ? testList("codex-router-cli") : ""}`;
  return [
    ...errors,
    ...requireMarkers({
      label: harness ? "installed-Codex harness inventory" : "permanent test inventory",
      source,
      markers: inventoryByRow[row] ?? [],
    }),
  ];
}
function authBoundaryErrors(): string[] {
  const errors: string[] = [];
  const common = [
    "refresh_credentials",
    "write_secret",
    "read_secret",
    "account_credential_bundle_key",
    "activate_account_credential_generation_if_current_and_invalidate_quota",
    "rusqlite",
    "sqlx::",
  ];
  const forbidden = [
    "codex_router_secret_store::",
    "SecretStore",
    "OpenAiOAuthRefreshClient",
    "CredentialRefreshClient",
    "RouterCredentialResolver",
    "AsyncRouterCredentialResolver",
    "RefreshLeaseRegistry",
    "AsyncRefreshLeaseRegistry",
    ...common,
  ];
  for (const path of [releaseFiles[0], releaseFiles[3], releaseFiles[1], releaseFiles[2]]) {
    if (path === undefined) throw new Error("release path absent");
    const source = read(path);
    for (const marker of forbidden)
      if (source.includes(marker))
        errors.push(`${path} contains request-time auth/state boundary marker: ${marker}`);
  }
  for (const path of globSync("crates/codex-router-proxy/src/*.rs").sort()) {
    if (path === "crates/codex-router-proxy/src/lib.rs") continue;
    const source = read(path);
    for (const marker of common)
      if (source.includes(marker))
        errors.push(`${path} contains request-time auth/state boundary marker: ${marker}`);
  }
  errors.push(
    ...requireMarkers({
      label: "release serve auth/state boundary",
      source: read("crates/codex-router-proxy/src/server.rs"),
      markers: [
        "ProxyRuntimeCredentialResources::open",
        "AsyncProxyCredentialResolverFactory",
        ".resolver_for_state(",
      ],
    }),
  );
  errors.push(
    ...requireMarkers({
      label: "auth-owned credential resolver",
      source: read("crates/codex-router-auth/src/resolver.rs"),
      markers: [
        "pub struct AsyncRouterCredentialResolver",
        "pub struct AsyncRefreshLeaseRegistry",
        "refresh_client.refresh_credentials",
        ".write_secret(",
        ".activate_account_credential_generation_if_current_and_invalidate_quota(",
      ],
    }),
  );
  return errors;
}
function servingErrors(row: string): string[] {
  const server = read("crates/codex-router-proxy/src/server.rs");
  const websocket = read("crates/codex-router-proxy/src/websocket.rs");
  const errors: string[] = [];
  if (row === "G-08") {
    for (const marker of [
      "tungstenite::accept",
      "accept_hdr_async",
      "derive_accept_key",
      "BlockingWebSocketTunnel",
    ])
      if (server.includes(marker))
        errors.push(
          `release serve switchpoint still contains alternate parser/handshake marker: ${marker}`,
        );
    errors.push(
      ...requireMarkers({
        label: "release serve switchpoint",
        source: server,
        markers: [
          "hyper_tungstenite::upgrade",
          "AsyncWebSocketTunnel",
          "handle_upgraded_connection",
        ],
      }),
    );
  } else if (row === "G-11") {
    const cli = read("crates/codex-router-cli/src/lib.rs");
    errors.push(
      ...requireMarkers({
        label: "CLI serve path",
        source: cli,
        markers: ["LoopbackRouterRuntime::start", "serve_protocol_connections"],
      }),
    );
    if (cli.includes("LoopbackHttpServer::bind") || cli.includes("LoopbackHttpAdapter::handle"))
      errors.push("CLI serve path still references legacy blocking loopback server");
    if (
      websocket.includes("pub struct BlockingWebSocketTunnel") &&
      !websocket.includes("#[cfg(test)]")
    )
      errors.push("blocking websocket tunnel is not visibly test-gated");
    errors.push(
      ...requireMarkers({
        label: "server handler lifecycle",
        source: server,
        markers: ["JoinSet", "handle_connection_join_result"],
      }),
    );
  } else if (row === "G-12") {
    for (const marker of ["unbounded_channel", "mpsc::unbounded"])
      if (websocket.includes(marker) || server.includes(marker))
        errors.push(`runtime contains unbounded channel marker: ${marker}`);
    errors.push(
      ...requireMarkers({
        label: "affinity write tracker",
        source: websocket,
        markers: ["TaskTracker"],
      }),
      ...requireMarkers({
        label: "affinity write tracker",
        source: server,
        markers: ["affinity_record_tasks.wait().await"],
      }),
    );
  } else {
    for (const marker of ["unbounded_channel", "mpsc::unbounded", "std::sync::mpsc"])
      if (websocket.includes(marker))
        errors.push(`WebSocket pump contains unbounded channel marker: ${marker}`);
    errors.push(
      ...requireMarkers({
        label: "WebSocket pump",
        source: websocket,
        markers: ["tokio::select!", "forward_duplex_until_complete"],
      }),
    );
  }
  return errors;
}
export function structuralErrors(row: string, runner: CommandRunner = runProcess): string[] {
  if (inventoryByRow[row] !== undefined) return inventoryErrors(row, runner);
  if (row === "G-09") return resolverErrors(runner);
  if (row === "G-29") return authBoundaryErrors();
  if (row === "G-06") {
    const errors: string[] = [];
    for (const path of releaseFiles) {
      const source = read(path);
      if (source.includes("rusqlite") || /(?<!Async)SqliteStateStore::open/.test(source))
        errors.push(`${path} contains direct sync SQLite access marker`);
    }
    return [
      ...errors,
      ...requireMarkers({
        label: "server runtime",
        source: read("crates/codex-router-proxy/src/server.rs"),
        markers: ["AsyncSqliteStateStore::open"],
      }),
    ];
  }
  if (row === "G-10") {
    const proxy = read("crates/codex-router-proxy/Cargo.toml");
    const errors = [
      ...requireMarkers({
        label: "workspace manifest",
        source: read("Cargo.toml"),
        markers: ["tokio-tungstenite"],
      }),
      ...requireMarkers({
        label: "proxy manifest",
        source: proxy,
        markers: ["tokio-tungstenite.workspace = true"],
      }),
    ];
    if (/^\s*tungstenite\s*=/m.test(proxy))
      errors.push("proxy production manifest declares blocking tungstenite directly");
    return errors;
  }
  if (["G-08", "G-11", "G-12", "G-22"].includes(row)) {
    const errors = servingErrors(row);
    if (row === "G-12") {
      const result = runner(
        [
          "cargo",
          "test",
          "-p",
          "codex-router-proxy",
          "async_websocket_tunnel_does_not_gate_forwarding_on_slow_affinity_recorder",
          "--",
          "--nocapture",
        ],
        {},
      );
      if (result.status !== 0)
        errors.push(
          `slow affinity recorder behavioral proof failed: ${result.stderr.trim() || result.stdout.trim()}`,
        );
    }
    return errors;
  }
  return [];
}
