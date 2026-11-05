import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  statSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";

function requireRecord(value: unknown): Record<string, unknown> {
  assert.ok(typeof value === "object" && value !== null && !Array.isArray(value));
  return value as Record<string, unknown>;
}
const evidence = "tmp/test-org-measure";
const snapshot = "tmp/test-org-cached-target";
const testFile = "crates/collaboration-service/src/mail_removal_backfill_test.rs";
const productFile = "crates/collaboration-service/src/lib.rs";
const filter =
  "not((package(=agent-collaboration) & binary(=e2e)) | (package(=board-legacy-upgrade) & binary(=e2e)) | (package(=collaboration-central-tokio) & binary(=e2e) & test(/^forwarding_real_timer::/)))";
const build = [
  "nextest",
  "run",
  "--profile",
  "ci",
  "--status-level",
  "all",
  "--final-status-level",
  "all",
  "--workspace",
  "--features",
  "codex-router-cli/keychain-test-support",
  "--exclude",
  "codex-router-quota-reset-test-harness",
  "-E",
  filter,
  "--no-run",
];
function runCheckedCommand(program: string, args: string[]): string {
  const result = spawnSync(program, args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (result.status !== 0) throw new Error(`${program} exited ${result.status}: ${result.stderr}`);
  return result.stdout;
}
function measureBuildScenario(name: string): void {
  const started = new Date().toISOString();
  const measured = spawnSync(
    "/usr/bin/time",
    ["-v", "-o", `${evidence}/${name}-time.txt`, "cargo", ...build],
    { stdio: "inherit" },
  );
  writeFileSync(`${evidence}/${name}.exit`, String(measured.status ?? -1) + "\n");
  if (measured.status !== 0) throw new Error(`${name} build failed: ${measured.status}`);
  const bytes = Number(runCheckedCommand("du", ["-sb", "target/debug"]).split(/\s+/)[0]);
  const time = readFileSync(`${evidence}/${name}-time.txt`, "utf8");
  const parseTimeMetric = (label: string): number => {
    const row = time.split("\n").find((line) => line.trim().startsWith(label + ":"));
    assert.ok(row, `missing ${label}`);
    return Number(
      row
        .trim()
        .slice(label.length + 1)
        .trim(),
    );
  };
  const elapsedRow = time.split("\n").find((line) => line.includes("Elapsed (wall clock) time"));
  assert.ok(elapsedRow);
  const elapsed = elapsedRow
    .slice(elapsedRow.indexOf("):") + 2)
    .trim()
    .split(":")
    .map(Number);
  const seconds = elapsed.reduce((sum, part) => sum * 60 + part, 0);
  assert.ok(Number.isFinite(bytes) && bytes > 0, "target-byte measurement is invalid");
  assert.ok(Number.isFinite(seconds) && seconds > 0, "elapsed measurement is invalid");
  writeFileSync(
    `${evidence}/${name}.json`,
    JSON.stringify(
      {
        scenario: name,
        started,
        completed: new Date().toISOString(),
        command: ["cargo", ...build],
        exit: 0,
        userSeconds: parseTimeMetric("User time (seconds)"),
        systemSeconds: parseTimeMetric("System time (seconds)"),
        elapsedSeconds: seconds,
        targetDebugBytes: bytes,
      },
      null,
      2,
    ) + "\n",
  );
}
mkdirSync(evidence, { recursive: true });
const phase = process.argv[2];
if (phase === "prepare") {
  assert.equal(os.cpus().length, 4, "measurement target must be the GitHub four-vCPU runner");
  const available = Number(
    runCheckedCommand("df", ["--output=avail", "-B1", "."]).trim().split(/\s+/).at(-1),
  );
  assert.ok(
    available >= 60 * 1024 ** 3,
    "measurement needs 60 GiB for the cached target checkpoint",
  );
  const manifestPath = "crates/collaboration-service/Cargo.toml";
  const manifest = readFileSync(manifestPath, "utf8");
  assert.ok(!manifest.includes("[package.metadata.test-organization-measurement]"));
  const qualifier = process.env.GITHUB_RUN_ID;
  assert.ok(qualifier);
  // The normal action hashes parsed manifests, so metadata (not a comment) qualifies the entry.
  // Dependency topology and compiled sources stay unchanged.
  writeFileSync(
    manifestPath,
    manifest + `\n[package.metadata.test-organization-measurement]\nsample = "${qualifier}"\n`,
  );
  const states = Object.fromEntries(
    [testFile, productFile].map((file) => {
      const stat = statSync(file);
      return [
        file,
        { contents: readFileSync(file, "utf8"), atime: stat.atimeMs, mtime: stat.mtimeMs },
      ];
    }),
  );
  writeFileSync(`${evidence}/source-state.json`, JSON.stringify(states));
  writeFileSync(
    `${evidence}/target.json`,
    JSON.stringify(
      {
        head: runCheckedCommand("git", ["rev-parse", "HEAD"]).trim(),
        runId: qualifier,
        cpuCount: os.cpus().length,
        cpuModel: os.cpus()[0]?.model,
        platform: os.platform(),
        arch: os.arch(),
        runnerImage: process.env.ImageOS,
        runnerImageVersion: process.env.ImageVersion,
        rustc: runCheckedCommand("rustc", ["-Vv"]),
        nextest: runCheckedCommand("cargo", ["nextest", "--version"]),
        incremental: process.env.CARGO_INCREMENTAL,
        devDebug: process.env.CARGO_PROFILE_DEV_DEBUG,
        testDebug: process.env.CARGO_PROFILE_TEST_DEBUG,
        selection: build,
        cacheQualifier:
          "runtime-only package metadata marker; no dependency or compiled-source change",
      },
      null,
      2,
    ) + "\n",
  );
} else if (phase === "cold") {
  assert.ok(!existsSync("target/debug"), "cold scenario must start without target artifacts");
  measureBuildScenario("cold");
} else if (phase === "cached") {
  measureBuildScenario("cached-dependency");
  cpSync("target", snapshot, { recursive: true, preserveTimestamps: true });
} else if (phase === "test-edit") {
  const states = JSON.parse(readFileSync(`${evidence}/source-state.json`, "utf8"));
  writeFileSync(
    testFile,
    states[testFile].contents + "\n// Test-only build-cost measurement edit.\n",
  );
  measureBuildScenario("test-only-edit");
} else if (phase === "production-edit") {
  const states = JSON.parse(readFileSync(`${evidence}/source-state.json`, "utf8"));
  for (const file of [testFile, productFile]) {
    writeFileSync(file, states[file].contents);
    utimesSync(file, states[file].atime / 1000, states[file].mtime / 1000);
  }
  runCheckedCommand("cargo", ["clean", "--target-dir", "target"]);
  cpSync(snapshot, "target", { recursive: true, preserveTimestamps: true });
  writeFileSync(
    productFile,
    states[productFile].contents + "\n// Production-source build-cost measurement edit.\n",
  );
  measureBuildScenario("production-edit");
  for (const file of [testFile, productFile]) {
    writeFileSync(file, states[file].contents);
    utimesSync(file, states[file].atime / 1000, states[file].mtime / 1000);
  }
  const list = runCheckedCommand("cargo", [
    "nextest",
    "list",
    "--profile",
    "ci",
    "--workspace",
    "--features",
    "codex-router-cli/keychain-test-support",
    "--exclude",
    "codex-router-quota-reset-test-harness",
    "-E",
    filter,
    "--verbose",
    "--message-format",
    "json",
  ]);
  writeFileSync(`${evidence}/inventory.json`, list);
  const data = JSON.parse(list);
  const identities = [];
  for (const [id, suiteValue] of Object.entries(requireRecord(data["rust-suites"]))) {
    const suite = requireRecord(suiteValue);
    if (suite.status === "skipped") identities.push([id, "<binary-skipped>"]);
    for (const [name, testValue] of Object.entries(requireRecord(suite.testcases ?? {}))) {
      const test = requireRecord(testValue);
      identities.push([id, name, test["filter-match"], test.ignored]);
    }
  }
  identities.sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
  writeFileSync(
    `${evidence}/inventory-identities.json`,
    JSON.stringify(identities, null, 2) + "\n",
  );
  writeFileSync(
    `${evidence}/inventory-sha256.txt`,
    createHash("sha256").update(JSON.stringify(identities)).digest("hex") + "\n",
  );
} else throw new Error("unknown phase");
