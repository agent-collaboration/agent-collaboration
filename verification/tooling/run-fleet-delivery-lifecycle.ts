import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import {
  deliveryLifecycleCases,
  requireDeliveryDiscovery,
  requireDeliveryOutcomes,
} from "./fleet-delivery-lifecycle-contract.ts";

const repository = fileURLToPath(new URL("../../", import.meta.url));
const usage =
  "Usage: bash scripts/verification/delivery-lifecycle.sh <fresh-evidence-directory>\nRuns exactly seven native Turso lifecycle identities through signed nextest.\n";
const arguments_ = process.argv.slice(2);
if (arguments_.length === 1 && ["--help", "-h"].includes(arguments_[0] ?? "")) {
  process.stdout.write(usage);
  process.exit(0);
}
if (arguments_.length !== 1 || arguments_[0]?.startsWith("-")) {
  process.stderr.write(usage);
  process.exit(2);
}
const evidence = resolve(repository, arguments_[0] ?? "");
const environment = { ...process.env };
delete environment.GITHUB_ACTIONS;
const normalTarget = resolve(repository, "target");
assert(
  !environment.CARGO_TARGET_DIR || resolve(environment.CARGO_TARGET_DIR) === normalTarget,
  "use the declared normal target",
);
assert(
  !Object.keys(environment).some((name) => /^CARGO_TARGET_.*_RUNNER$/.test(name)),
  "runner override is not a declared signing route",
);
environment.CARGO_TARGET_DIR = normalTarget;
environment.CARGO_BUILD_JOBS ??= "2";
mkdirSync(evidence);

function command(executable: string, args: readonly string[], log: string): string {
  const result = spawnSync(executable, args, {
    cwd: repository,
    env: environment,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  writeFileSync(resolve(evidence, log), `${result.stdout ?? ""}${result.stderr ?? ""}`);
  writeFileSync(
    resolve(evidence, `${log}.exit.json`),
    `${JSON.stringify({ command: [executable, ...args], status: result.status, signal: result.signal, error: result.error?.message ?? null })}\n`,
  );
  assert.equal(result.status, 0, `command failed; see ${log}`);
  return result.stdout;
}

function sourceState(): string {
  const head = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repository, encoding: "utf8" });
  const status = spawnSync("git", ["status", "--porcelain"], { cwd: repository, encoding: "utf8" });
  const diff = spawnSync("git", ["diff", "HEAD"], {
    cwd: repository,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  assert.equal(head.status, 0);
  assert.equal(status.status, 0);
  assert.equal(diff.status, 0);
  const apparatus = [
    "scripts/verification/delivery-lifecycle.sh",
    "verification/tooling/run-fleet-delivery-lifecycle.ts",
    "verification/tooling/fleet-delivery-lifecycle-contract.ts",
    ".cargo/config.toml",
    "scripts/cargo_debug_signing_runner.sh",
  ].map((file) => ({
    file,
    sha256: createHash("sha256")
      .update(readFileSync(resolve(repository, file)))
      .digest("hex"),
  }));
  return JSON.stringify({
    head: head.stdout.trim(),
    status: status.stdout,
    diffSha256: createHash("sha256").update(diff.stdout).digest("hex"),
    apparatus,
  });
}

function signDiscoveredBinaries(binaries: readonly string[]): void {
  if (process.platform !== "darwin") return;
  const runner = resolve(repository, "scripts/cargo_debug_signing_runner.sh");
  for (const binary of binaries) {
    assert(
      resolve(binary).startsWith(`${normalTarget}${sep}`),
      "discovered binary outside normal target",
    );
    assert.equal(realpathSync(binary), resolve(binary), "discovered binary must not be a symlink");
  }
  command("bash", [runner, "--sign-only", ...binaries], "signing.log");
  for (const [index, binary] of binaries.entries()) {
    command("codesign", ["--verify", "--strict", binary], `signature-${index}.log`);
    command("codesign", ["-dv", "--verbose=4", binary], `team-${index}.log`);
    assert(
      readFileSync(resolve(evidence, `team-${index}.log`), "utf8")
        .split("\n")
        .includes("TeamIdentifier=974QD84WVC"),
      "unexpected signing team",
    );
  }
}

try {
  assert(
    readFileSync(resolve(repository, ".cargo/config.toml"), "utf8").includes(
      'runner = "scripts/cargo_debug_signing_runner.sh"',
    ),
    "missing repository signing runner",
  );
  const before = sourceState();
  writeFileSync(resolve(evidence, "source-before.json"), `${before}\n`);
  const disk = command("df", ["-k", repository], "disk.log");
  const available = Number(disk.trim().split("\n").at(-1)?.trim().split(/\s+/)[3]);
  assert(Number.isFinite(available) && available >= 31_457_280, "less than 30 GiB free");
  const filter = deliveryLifecycleCases.map((row) => `test(=${row.test})`).join(" | ");
  const selection = [
    "--locked",
    "-p",
    "fleet-mail-delivery",
    "--all-features",
    "--lib",
    "--test",
    "integration",
    "-E",
    filter,
  ];
  const discovery = command(
    "cargo",
    ["nextest", "list", ...selection, "--message-format", "json"],
    "discovery.log",
  );
  const binaries = requireDeliveryDiscovery(JSON.parse(discovery));
  signDiscoveredBinaries(binaries);
  writeFileSync(
    resolve(evidence, "binary-sha256.json"),
    `${JSON.stringify(
      binaries.map((file) => ({
        file,
        sha256: createHash("sha256").update(readFileSync(file)).digest("hex"),
      })),
      null,
      2,
    )}\n`,
  );
  command(
    "cargo",
    [
      "nextest",
      "run",
      ...selection,
      "--no-tests",
      "fail",
      "--test-threads",
      "1",
      "--color",
      "never",
      "--status-level",
      "none",
      "--final-status-level",
      "all",
    ],
    "execution.log",
  );
  const outcomes = requireDeliveryOutcomes(
    readFileSync(resolve(evidence, "execution.log"), "utf8"),
  );
  const after = sourceState();
  writeFileSync(resolve(evidence, "source-after.json"), `${after}\n`);
  assert.equal(after, before, "source changed during delivery observation");
  writeFileSync(
    resolve(evidence, "complete.json"),
    `${JSON.stringify({ cases: outcomes, passed: outcomes.length, scope: "native Turso APIs and durable engines; no hosted delivery acceptance" }, null, 2)}\n`,
  );
  process.stdout.write(
    `${outcomes.length} exact delivery lifecycle cases passed; evidence: ${evidence}\n`,
  );
} catch (error) {
  writeFileSync(
    resolve(evidence, "failure.txt"),
    `${error instanceof Error ? error.stack : String(error)}\n`,
  );
  process.stderr.write(`Delivery lifecycle observation incomplete; see ${evidence}\n`);
  process.exitCode = 2;
}
