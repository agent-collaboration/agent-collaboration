import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import {
  closeSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  openSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { z } from "zod";
import {
  caseIdentities,
  requireCaseResults,
} from "../fixtures/fleet-copy-workerd/case-contracts.ts";

const repository = fileURLToPath(new URL("../../", import.meta.url));
const usage =
  "Usage: node verification/tooling/run-fleet-workerd.ts <fresh-evidence-directory>\nSet WASM_BINDGEN to wasm-bindgen CLI 0.2.129. Runs all 31 adapter and 7 raw-SQL cases.\n";
const arguments_ = process.argv.slice(2);
if (arguments_.length === 1 && ["--help", "-h"].includes(arguments_[0] ?? "")) {
  process.stdout.write(usage);
  process.exit(0);
}
if (arguments_.length !== 1 || arguments_[0]?.startsWith("-")) {
  process.stderr.write(usage);
  process.exit(2);
}
const evidence = resolve(z.string().min(1).parse(process.argv[2]));
const worker = resolve(repository, "crates/fleet-project-copy/tests/workerd");
const manifest = resolve(worker, "Cargo.toml");
const bindgen =
  process.env.WASM_BINDGEN ??
  execFileSync("/usr/bin/which", ["wasm-bindgen"], { encoding: "utf8" }).trim();
assert.ok(
  !existsSync(evidence) || readdirSync(evidence).length === 0,
  "use a fresh empty evidence directory",
);
mkdirSync(evidence, { recursive: true });
interface StageReceipt {
  name: string;
  command: string;
  arguments: readonly string[];
  elapsedSeconds: number;
  exitCode: number | null;
}
interface Stage {
  name: string;
  command: string;
  arguments: readonly string[];
  environment?: NodeJS.ProcessEnv;
}
const receipts: StageReceipt[] = [];
const started = performance.now();
function runStage(stage: Stage): void {
  const log = openSync(resolve(evidence, stage.name + ".log"), "w");
  const at = performance.now();
  try {
    const result = spawnSync(stage.command, [...stage.arguments], {
      cwd: repository,
      env: { ...process.env, ...stage.environment },
      stdio: ["ignore", log, log],
    });
    receipts.push({
      name: stage.name,
      command: stage.command,
      arguments: stage.arguments,
      elapsedSeconds: (performance.now() - at) / 1000,
      exitCode: result.status,
    });
    writeFileSync(resolve(evidence, "stages.json"), JSON.stringify(receipts, null, 2) + "\n");
    if (result.error) throw result.error;
    assert.equal(
      result.status,
      0,
      `${stage.name} failed; inspect ${resolve(evidence, stage.name + ".log")}`,
    );
    console.log(JSON.stringify(receipts.at(-1)));
  } finally {
    closeSync(log);
  }
}
function cargoStage(
  name: string,
  arguments_: readonly string[],
  environment?: NodeJS.ProcessEnv,
): void {
  runStage({ name: name + "-disk", command: "df", arguments: ["-Pk", repository] });
  const disk = readFileSync(resolve(evidence, name + "-disk.log"), "utf8")
    .trim()
    .split("\n")
    .at(-1);
  const freeKiB = z.coerce.number().positive().parse(disk?.split(/\s+/)[3]);
  assert.ok(freeKiB >= 30 * 1024 * 1024, "build stopped: less than 30 GiB available");
  const stage = { name, command: "cargo", arguments: arguments_ };
  runStage(environment === undefined ? stage : { ...stage, environment });
}
const native = [
  "nextest",
  "run",
  "--locked",
  "-p",
  "fleet-native-services",
  "--features",
  "test-support,test-fault",
  "--test-threads",
  "1",
  "--status-level",
  "all",
  "--final-status-level",
  "all",
];
try {
  // Never reuse stale fixture exports as proof of the current native writer.
  assert.ok(
    !readFileSyncSafe(resolve(evidence, "complete.json")),
    "use a fresh evidence directory",
  );
  cargoStage(
    "export-renames",
    [...native, "--test", "e2e", "-E", "test(/^native_migration_rename::/)"],
    {
      NORMAL_RUN_ROOT: resolve(evidence, "native-renames"),
      NORMAL_FIXTURE_ROOT: resolve(evidence, "rename-fixtures"),
    },
  );
  const certainty = resolve(evidence, "certainty.json");
  cargoStage(
    "export-certainty",
    [
      ...native,
      "--test",
      "e2e",
      "-E",
      "test(=native_copy_containment::an_unknown_commit_reconciles_the_identical_piece_without_a_second_effect)",
    ],
    { COPY_CERTAINTY_FIXTURE: certainty },
  );
  const foreignKeys = resolve(evidence, "foreign-keys");
  mkdirSync(foreignKeys, { recursive: true });
  cargoStage(
    "export-foreign-key-rules",
    [
      "nextest",
      "run",
      "--locked",
      "-p",
      "fleet-project-copy",
      "--lib",
      "--test-threads",
      "1",
      "-E",
      "test(turso_foreign_key)",
      "--status-level",
      "all",
      "--final-status-level",
      "all",
    ],
    { FK_FIXTURE_ROOT: foreignKeys },
  );
  cargoStage(
    "export-writer-order",
    [...native, "--test", "integration", "-E", "test(/^native_receiver_foreign_keys::/)"],
    { FK_CAPTURE_FIXTURE: resolve(foreignKeys, "captured.json") },
  );
  cargoStage("worker-feature-tree", [
    "tree",
    "--locked",
    "--manifest-path",
    manifest,
    "--target",
    "wasm32-unknown-unknown",
    "--edges",
    "normal,build,features",
  ]);
  cargoStage("worker-build", [
    "build",
    "--locked",
    "--manifest-path",
    manifest,
    "--target",
    "wasm32-unknown-unknown",
  ]);
  const runner = resolve(repository, "scripts/cargo_debug_signing_runner.sh");
  runStage({ name: "bindgen-version", command: runner, arguments: [bindgen, "--version"] });
  assert.match(
    readFileSync(resolve(evidence, "bindgen-version.log"), "utf8"),
    /^wasm-bindgen 0\.2\.129\s*$/,
  );
  runStage({
    name: "worker-bindings",
    command: runner,
    arguments: [
      bindgen,
      resolve(worker, "target/wasm32-unknown-unknown/debug/fleet_copy_workerd_fixture.wasm"),
      "--target",
      "bundler",
      "--out-name",
      "index",
      "--out-dir",
      resolve(worker, "build"),
    ],
  });
  copyFileSync(resolve(worker, "shim.mjs"), resolve(worker, "build/shim.mjs"));
  const matrix = resolve(evidence, "matrix.json");
  runStage({
    name: "prepare-matrix",
    command: process.execPath,
    arguments: [
      resolve(repository, "verification/fixtures/fleet-copy-workerd/prepare-matrix.ts"),
      matrix,
      resolve(evidence, "rename-fixtures"),
    ],
  });
  for (const [name, controller, arguments_] of [
    ["rename", "fleet-copy-workerd/run-matrix.ts", [matrix, resolve(evidence, "rename")]],
    [
      "certainty",
      "fleet-copy-workerd/run-copy-certainty.ts",
      [certainty, resolve(evidence, "certainty")],
    ],
    [
      "foreignKeys",
      "fleet-copy-workerd/run-foreign-keys.ts",
      [foreignKeys, resolve(evidence, "foreignKeys")],
    ],
    [
      "migrationSql",
      "fleet-migrations-workerd/public-migration-sql.ts",
      [resolve(evidence, "migrationSql")],
    ],
  ] satisfies Array<[string, string, string[]]>) {
    runStage({
      name,
      command: process.execPath,
      arguments: [resolve(repository, "verification/fixtures", controller), ...arguments_],
    });
  }
  for (const group of ["rename", "certainty", "foreignKeys", "migrationSql"] as const) {
    const results: unknown = JSON.parse(
      readFileSync(resolve(evidence, group, "results.json"), "utf8"),
    );
    if (group === "rename") {
      const supplied = z
        .object({ snapshots: z.array(z.object({ name: z.string() })).optional() })
        .parse(JSON.parse(readFileSync(matrix, "utf8")));
      const rows = z.array(z.looseObject({ name: z.string(), kind: z.string() })).parse(results);
      requireCaseResults(
        rows.filter((row) => caseIdentities.rename.includes(row.name)),
        caseIdentities.rename,
      );
      const snapshots = supplied.snapshots ?? [];
      const snapshotRows = rows.filter((row) => !caseIdentities.rename.includes(row.name));
      if (snapshots.length)
        requireCaseResults(
          snapshotRows,
          snapshots.map((row) => row.name),
          "snapshotPassed",
        );
      else assert.deepEqual(snapshotRows, []);
    } else requireCaseResults(results, caseIdentities[group]);
  }
  writeFileSync(
    resolve(evidence, "complete.json"),
    JSON.stringify(
      {
        adapterCases: 31,
        rawSqlCases: 7,
        elapsedSeconds: (performance.now() - started) / 1000,
        scope: "local workerd; whole combined CI duration is recorded separately",
      },
      null,
      2,
    ) + "\n",
  );
} finally {
  writeFileSync(resolve(evidence, "stages.json"), JSON.stringify(receipts, null, 2) + "\n");
}
function readFileSyncSafe(path: string): string | undefined {
  try {
    return readFileSync(path, "utf8");
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return undefined;
    throw error;
  }
}
