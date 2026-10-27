import { readFileSync } from "node:fs";
import { z } from "zod";
import { isDeepStrictEqual } from "node:util";
import {
  readProof,
  writeProof,
  record,
  section,
  truthy,
  equalNumber,
  atLeast,
  list,
  display,
  displayText,
  sourcePaths,
  guardedPathsDirty,
  copyArtifact,
  settleReceipt,
  type ProofRecord,
} from "./proof-artifact-values.ts";
import { runProcess, type CommandRunner } from "./process-command.ts";
import { failed, soakRowErrors } from "./proof-soak-checks.ts";
import { structuralErrors, httpReleaseErrors } from "./proof-structural-checks.ts";

export function guardClean(row: string, guard: string, runner: CommandRunner = runProcess): number {
  const paths = sourcePaths(guard);
  if (paths.length === 0) {
    console.error(`proof row ${row} has no guarded paths`);
    return 1;
  }
  if (guardedPathsDirty(paths, runner)) {
    console.error(
      `proof row ${row} guarded source paths are dirty; commit or revert guarded paths before accepting this receipt`,
    );
    return 1;
  }
  return 0;
}
function commonSuccess(): ProofRecord {
  return {
    status_after: "[x] passed",
    result: "pass",
    exit_code: 0,
    freshness_check: "guarded_source_paths_clean_at_git_head",
  };
}
export function patchReceipt(path: string, row: string): void {
  const patches = readProof(new URL("proof-receipt-patches.json", import.meta.url).pathname);
  writeProof(path, { ...readProof(path), ...record(patches[row]) });
}
export function passReceipt(path: string, notes: string): void {
  writeProof(path, { ...readProof(path), ...commonSuccess(), notes });
}
export function accountsReceipt(paths: {
  readonly receipt: string;
  readonly serial: string;
  readonly concurrent: string;
}): number {
  const receipt = readProof(paths.receipt);
  const serial = readProof(paths.serial);
  const concurrent = readProof(paths.concurrent);
  const clients = section(concurrent, "clients");
  const selected = section(concurrent, "selected_account");
  const upstream = section(concurrent, "upstream");
  const errors = failed([
    [
      section(serial, "websocket").selected_expected_account === true,
      "serial WebSocket artifact did not prove expected account selection",
    ],
    [
      truthy(section(serial, "selected_account").safe_tag),
      "serial WebSocket artifact did not include selected account safe tag",
    ],
    [equalNumber(clients.count, 3), "concurrent artifact did not run three clients"],
    [clients.all_success === true, "concurrent artifact did not report all clients successful"],
    [
      selected.expected_upstream_account_selected === true,
      "concurrent artifact did not prove selected upstream account evidence",
    ],
    [truthy(selected.safe_tag), "concurrent artifact did not include selected account safe tag"],
    [
      list(upstream.upstream_client_sessions).length >= 3,
      "concurrent artifact did not record upstream client sessions",
    ],
    [
      atLeast(upstream.active_high_water, 3),
      "concurrent artifact did not prove overlapping WebSocket sessions",
    ],
  ]);
  const artifactPaths = [
    copyArtifact(paths.serial, paths.receipt, "AR-E2E-ACCOUNTS-serial-transcript.json"),
    copyArtifact(paths.concurrent, paths.receipt, "AR-E2E-ACCOUNTS-concurrent-transcript.json"),
  ];
  Object.assign(receipt, {
    freshness_check: "guarded_source_paths_clean_at_git_head",
    artifact_paths: artifactPaths,
    notes:
      errors.length === 0
        ? "Serial and three-client concurrent installed-Codex WebSocket transcripts prove selected-account routing and transport continuity."
        : errors.join("; "),
  });
  return settleReceipt(paths.receipt, receipt, errors);
}
export function serialReceipt(input: {
  readonly row: string;
  readonly receipt: string;
  readonly artifact: string;
  readonly output: string;
}): number {
  const receipt = readProof(input.receipt);
  const artifact = readProof(input.artifact);
  const router = section(artifact, "router_process");
  const errors: string[] = [];
  if (input.row === "S-03") {
    for (const field of [
      "binary_path",
      "pid",
      "argv",
      "listener",
      "readiness_line",
      "cleanup_result",
    ])
      if (
        router[field] === undefined ||
        router[field] === null ||
        router[field] === "" ||
        isDeepStrictEqual(router[field], [])
      )
        errors.push(`router_process.${field} missing from serial smoke artifact`);
    if (router.spawned_real_serve_child !== true)
      errors.push("router_process.spawned_real_serve_child is not true");
  }
  if (input.row === "S-04") {
    const text = readFileSync(input.artifact, "utf8").toLowerCase();
    const output = readFileSync(input.output, "utf8").toLowerCase();
    for (const fragment of ["falling back", "fallback", "reconnecting", "request timed out"])
      if (text.includes(fragment) || output.includes(fragment))
        errors.push(
          `serial WebSocket smoke observed forbidden transport degradation text: ${fragment}`,
        );
    if (!atLeast(section(artifact, "websocket").request_frame_count, 1))
      errors.push("serial WebSocket smoke did not record upstream request frames");
    if (!equalNumber(section(artifact, "upstream").handshake_count, 1))
      errors.push("serial WebSocket smoke did not record exactly one upstream handshake");
  }
  const notes: Readonly<Record<string, string>> = {
    "S-01": "Installed Codex WebSocket smoke passed with tokenless spawned router profile.",
    "S-03":
      "Installed Codex WebSocket smoke passed through spawned built codex-router serve child.",
    "S-04":
      "Installed Codex WebSocket mock upstream smoke passed deterministic runtime/no-fallback contract.",
  };
  Object.assign(receipt, {
    touched_targets: [
      "crates/codex-router-test-support/src/installed_codex.rs",
      "tests/smoke/installed_codex_mock.sh",
      "crates/codex-router-cli/src/lib.rs",
      "crates/codex-router-proxy/src/server.rs",
      "crates/codex-router-proxy/src/websocket.rs",
    ],
    freshness_check: "guarded_source_paths_clean_at_git_head",
    artifact_paths: [copyArtifact(input.artifact, input.receipt, `${input.row}-transcript.json`)],
    notes: errors.length === 0 ? z.string().parse(notes[input.row]) : errors.join("; "),
  });
  return settleReceipt(input.receipt, receipt, errors);
}
function soakCommonErrors(artifact: ProofRecord): string[] {
  const clients = section(artifact, "clients");
  const upstream = section(artifact, "upstream");
  const errors = failed([
    [artifact.mode === "three-websocket-soak", "artifact mode is not three-websocket-soak"],
    [truthy(clients.all_success), "clients did not all succeed"],
    [equalNumber(clients.count, 3), "client count is not 3"],
    [clients.target_model === "gpt-5.4-mini", "target client model is not gpt-5.4-mini"],
    [upstream.target_model === "gpt-5.4-mini", "upstream target_model is not gpt-5.4-mini"],
    [
      equalNumber(upstream.target_model_session_count, 3),
      "upstream target_model_session_count is not 3",
    ],
  ]);
  const models = upstream.unexpected_response_create_models;
  if (!Array.isArray(models)) errors.push("unexpected_response_create_models is not a list");
  else if (models.length !== 0)
    errors.push(`unexpected response.create models observed: ${display(models)}`);
  return errors;
}
export interface SoakReceiptInput {
  readonly row: string;
  readonly artifact: string;
  readonly receipt: string;
  readonly source: string;
  readonly guard: string;
}
export function soakReceipt(input: SoakReceiptInput, runner: CommandRunner = runProcess): number {
  const artifact = readProof(input.artifact);
  const receipt = readProof(input.receipt);
  const errors: string[] = [];
  const paths = sourcePaths(input.guard);
  if (guardedPathsDirty(paths, runner))
    errors.push(
      "guarded proof source paths are dirty; commit or revert them before accepting E-row proof",
    );
  if (
    artifact.git_head !== receipt.git_head &&
    runner(
      [
        "git",
        "diff",
        "--quiet",
        `${displayText(artifact.git_head)}..${displayText(receipt.git_head)}`,
        "--",
        ...paths,
      ],
      { inherit: true },
    ).status !== 0
  )
    errors.push(
      `artifact git_head ${displayText(artifact.git_head)} does not match current proof git_head ${displayText(receipt.git_head)} and relevant source paths changed`,
    );
  const codex = runner(["codex", "--version"], {});
  if (codex.status !== 0) errors.push("failed to read installed Codex version for E-row freshness");
  else if (artifact.codex_version !== codex.stdout.trim())
    errors.push(
      `artifact codex_version ${displayText(artifact.codex_version)} does not match installed Codex ${codex.stdout.trim()}`,
    );
  const rendered = readFileSync(input.artifact, "utf8");
  errors.push(...soakCommonErrors(artifact), ...soakRowErrors(input.row, artifact, rendered));
  Object.assign(receipt, {
    artifact_paths: [
      copyArtifact(input.artifact, input.receipt, "three-websocket-soak-transcript.json"),
    ],
    freshness_check:
      "explicit_artifact_pointer_and_git_head_match_current_head_or_relevant_source_unchanged",
    artifact_source: input.source,
    touched_targets: [
      "crates/codex-router-test-support/src/installed_codex.rs",
      "tests/smoke/installed_codex_mock.sh",
      "crates/codex-router-proxy/src/websocket.rs",
      "crates/codex-router-proxy/src/server.rs",
      "crates/codex-router-cli/src/lib.rs",
      "scripts/proof-matrix.sh",
    ],
    notes:
      errors.length === 0
        ? "Validated explicit installed-Codex three-WebSocket soak artifact."
        : errors.join("; "),
    soak_summary: {
      mode: artifact.mode ?? null,
      clients: artifact.clients ?? null,
      upstream: section(artifact, "upstream"),
      router_websocket_registry: section(artifact, "router_websocket_registry"),
      socket_cleanup: section(artifact, "socket_cleanup"),
    },
  });
  // The original performs the privacy scan after serializing its first receipt.
  Object.assign(receipt, {
    status_after: errors.length === 0 ? "[x] passed" : "[ ] pending",
    result: errors.length === 0 ? "pass" : "fail",
    exit_code: errors.length === 0 ? 0 : 1,
  });
  writeProof(input.receipt, receipt);
  if (input.row === "E-09") {
    const written = readFileSync(input.receipt, "utf8");
    for (const fragment of ["/Users/", "/var/folders/"])
      if (rendered.includes(fragment) || written.includes(fragment))
        errors.push(`proof pack leaked local absolute path fragment: ${fragment}`);
    if (errors.length !== 0) receipt.notes = errors.join("; ");
  }
  return settleReceipt(input.receipt, receipt, errors);
}
function structuralReceipt(row: string, path: string): number {
  const errors = structuralErrors(row);
  const receipt = readProof(path);
  Object.assign(receipt, {
    touched_targets: [
      "scripts/proof-matrix.sh",
      "verification/tooling/check-release-runtime-guardrails.ts",
      "crates/codex-router-proxy/src/websocket.rs",
      "crates/codex-router-proxy/src/server.rs",
      "crates/codex-router-proxy/src/credential_runtime.rs",
      "crates/codex-router-test-support/src/installed_codex.rs",
      "tests/smoke/installed_codex_mock.sh",
    ],
    freshness_check: "guarded_source_paths_clean_at_git_head",
    notes: errors.length === 0 ? "Structural/inventory guard passed." : errors.join("; "),
  });
  return settleReceipt(path, receipt, errors);
}
export function main(): number {
  const argument = (index: number): string => z.string().parse(process.argv[index]);
  switch (argument(2)) {
    case "guard":
      return guardClean(argument(3), argument(4));
    case "pass":
      passReceipt(argument(3), argument(4));
      return 0;
    case "patch":
      patchReceipt(argument(3), argument(4));
      return 0;
    case "accounts":
      return accountsReceipt({
        receipt: argument(3),
        serial: argument(4),
        concurrent: argument(5),
      });
    case "serial":
      return serialReceipt({
        row: argument(3),
        receipt: argument(4),
        artifact: argument(5),
        output: argument(6),
      });
    case "soak":
      return soakReceipt({
        row: argument(3),
        artifact: argument(4),
        receipt: argument(5),
        source: argument(6),
        guard: argument(7),
      });
    case "structural":
      return structuralReceipt(argument(3), argument(4));
    case "http": {
      const errors = httpReleaseErrors();
      for (const error of errors) console.log(error);
      return errors.length === 0 ? 0 : 1;
    }
    default:
      throw new Error("unknown proof receipt operation");
  }
}
if (import.meta.main) process.exitCode = main();
