import { execFileSync } from "node:child_process";
import { lstat, readFile, realpath } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { userInfo } from "node:os";
import { resolve } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { parse } from "smol-toml";
import { z } from "zod";
import { declarationSchema, type Declaration } from "./declaration.ts";
import { encode } from "./exact-json.ts";
import { casesFor, type RunnableCase } from "./case-registry.ts";
import {
  collectOutcomes,
  exitCode,
  readRows,
  resolveCoverage,
  type CaseOutcome,
} from "./coverage.ts";
import { ObservationOpen, Receipt } from "./receipt.ts";
import { prepareRun, type PreparedRun } from "./run-setup.ts";
import { RunLifetime } from "./run-lifetime.ts";

function lockGraph(text: string): unknown {
  const value = z
    .object({
      package: z.array(
        z
          .object({
            dependencies: z.array(z.string()).optional(),
          })
          .passthrough(),
      ),
    })
    .passthrough()
    .parse(parse(text));
  return {
    ...value,
    package: value.package.map((entry) =>
      entry.dependencies === undefined
        ? entry
        : { ...entry, dependencies: [...entry.dependencies].sort() },
    ),
  };
}

async function declarationFromArguments(): Promise<Declaration> {
  const path = process.argv[2];
  if (path === undefined || process.argv.length !== 3)
    throw new ObservationOpen("usage: node verification/run.ts PRIVATE_DECLARATION.json");
  const info = await lstat(path);
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o077) !== 0 ||
    info.size > 1024 * 1024
  )
    throw new ObservationOpen("bounded owner-private declaration required");
  // Configuration numbers are bounded ports/timeouts. Wire/state integers keep
  // using the lossless decoder in their owning clients.
  const declaration = declarationSchema.parse(JSON.parse(await readFile(path, "utf8")));
  const local = declaration.machines.find((machine): boolean => machine.kind === "local");
  if (local === undefined) throw new ObservationOpen("local installation missing");
  const cache = resolve(userInfo().homedir, ".cache");
  const roots = [
    declaration.evidenceDirectory,
    local.root,
    local.temporaryParent,
    ...(declaration.target.kind === "staging" ? [] : [declaration.target.root]),
  ];
  if (roots.some((root): boolean => resolve(root) !== root || !root.startsWith(`${cache}/`)))
    throw new ObservationOpen("run-owned state and evidence require canonical cache task roots");
  return declaration;
}
async function checkSource(declaration: Declaration): Promise<void> {
  if ((await realpath(fileURLToPath(new URL("..", import.meta.url)))) !== declaration.repository)
    throw new ObservationOpen("declaration repository differs from the running harness");
  const git = (arguments_: readonly string[]): string =>
    execFileSync("git", arguments_, {
      cwd: declaration.repository,
      encoding: "utf8",
      timeout: 10_000,
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
  // Discovery permits inherited unbuilt test/example/Python work. Executed
  // artifacts remain frozen by hash/signature. Confirmation requires the clean tree.
  const discoveryExclusions =
    declaration.purpose === "discovery"
      ? [
          ":(exclude)**/tests/**",
          ":(exclude)**/examples/**",
          ":(exclude)crates/codex-router-test-support",
          ":(exclude)scripts/verification/*.py",
          ":(exclude)Cargo.lock",
        ]
      : [];
  if (
    git(["rev-parse", "HEAD"]) !== declaration.harnessPin ||
    git([
      "status",
      "--porcelain",
      "--",
      ".",
      ":(exclude)docs",
      ":(exclude)tmp",
      ":(exclude).agents",
      ...discoveryExclusions,
    ]) !== ""
  )
    throw new ObservationOpen("harness source is not the declared immutable pin");
  git([
    "diff",
    "--quiet",
    declaration.productPin,
    declaration.harnessPin,
    "--",
    ".",
    ":(exclude)verification",
    ":(exclude)docs",
    ":(exclude)tmp",
    ":(exclude).agents",
    ...discoveryExclusions,
  ]);
  if (
    declaration.purpose === "discovery" &&
    !isDeepStrictEqual(
      lockGraph(git(["show", `${declaration.productPin}:Cargo.lock`])),
      lockGraph(await readFile(`${declaration.repository}/Cargo.lock`, "utf8")),
    )
  )
    throw new ObservationOpen("discovery dependency graph differs from the frozen product");
  if (Date.parse(declaration.deadline) <= Date.now())
    throw new ObservationOpen("declared run deadline has passed");
}
async function checkRun(context: PreparedRun): Promise<void> {
  for (const link of context.links) await link.assertAlive();
  for (const machine of context.machines) {
    await machine.installation.check();
    machine.recipient.connection.healthy();
  }
}
function failureDetails(error: unknown): unknown {
  return error instanceof Error
    ? { name: error.name, message: error.message, stack: error.stack, cause: error.cause }
    : { kind: "nonErrorThrown", type: typeof error };
}
async function executeCases(input: {
  declaration: Declaration;
  cases: readonly RunnableCase[];
  context: PreparedRun;
  receipt: Receipt;
  results: Map<string, CaseOutcome>;
}): Promise<void> {
  for (const scenario of input.cases.filter((entry): boolean =>
    entry.rows.some((row): boolean => input.declaration.rows.includes(row)),
  )) {
    if (Date.now() >= Date.parse(input.declaration.deadline)) {
      input.results.set(scenario.id, { kind: "open", reason: "declared run deadline reached" });
      break;
    }
    await input.receipt.record("caseStarted", { case: scenario.id, rows: scenario.rows });
    try {
      await checkRun(input.context);
      await scenario.run(input.context, input.receipt);
      await checkRun(input.context);
      if (Date.now() >= Date.parse(input.declaration.deadline))
        throw new ObservationOpen("case exceeded the declared run deadline");
      input.results.set(scenario.id, { kind: "observed" });
      await input.receipt.record("caseCompleted", { case: scenario.id });
    } catch (error: unknown) {
      await input.receipt.save(`case-${scenario.id}-failure.json`, failureDetails(error));
      input.results.set(
        scenario.id,
        error instanceof ObservationOpen
          ? { kind: "open", reason: "case observation unconfirmed" }
          : { kind: "failed", reason: "case assertion or required real-path operation failed" },
      );
      await input.receipt.record("caseStopped", {
        case: scenario.id,
        category: error instanceof ObservationOpen ? "unconfirmed" : "failure",
      });
    }
  }
}
async function main(): Promise<void> {
  const declaration = await declarationFromArguments();
  const receipt = new Receipt(declaration.evidenceDirectory);
  await receipt.begin(declaration);
  const cases = casesFor();
  const rows = readRows(declaration.vd);
  const mapping = resolveCoverage(rows, declaration.rows, cases);
  const results = new Map<string, CaseOutcome>();
  const missing = [...mapping]
    .filter(([, ids]): boolean => ids.length === 0)
    .map(([row]): string => row);
  await receipt.save("coverage.json", {
    selected: declaration.rows,
    mapping: Object.fromEntries(mapping),
    missing,
  });
  const lifetime = new RunLifetime(receipt);
  let cleanupFailures: readonly string[] = [];
  let setupUnconfirmed = false;
  if (missing.length === 0) {
    try {
      await checkSource(declaration);
      const context = await prepareRun({ declaration, receipt, lifetime });
      await executeCases({ declaration, cases, context, receipt, results });
      await checkSource(declaration);
    } catch (error: unknown) {
      setupUnconfirmed = true;
      await receipt.save("composition-failure.json", failureDetails(error));
      await receipt.record("runUnconfirmed", {
        phase: "source-or-composition",
        automaticReplay: false,
      });
    } finally {
      cleanupFailures = await lifetime.close();
    }
  }
  const outcomes = collectOutcomes({ rows, selected: declaration.rows, cases, results });
  let exit = exitCode(outcomes);
  if (exit === 0 && (setupUnconfirmed || cleanupFailures.length > 0)) exit = 2;
  await receipt.save("result.json", {
    purpose: declaration.purpose,
    releaseProofEligible: declaration.purpose === "confirmation" && exit === 0,
    selected: declaration.rows,
    outcomes: Object.fromEntries(outcomes),
    executedCases: results.size,
    caseResults: Object.fromEntries(results),
    cleanupFailures,
    setupUnconfirmed,
    configurationObservation: {
      evidence: "observations.jsonl",
      changedFilesStopCases: false,
      ownedWriteAttribution: "notEstablished",
      limit:
        "Owned open-file descriptors sampled; syscall writes not traced. Changes alone are notes.",
    },
    exit,
  });
  process.stdout.write(
    `${encode({
      purpose: declaration.purpose,
      releaseProofEligible: declaration.purpose === "confirmation" && exit === 0,
      selectedRows: declaration.rows.length,
      executedCases: results.size,
      unmappedRows: missing.length,
      cleanupComplete: cleanupFailures.length === 0,
      exit,
    })}\n`,
  );
  process.exitCode = exit;
}
try {
  await main();
} catch {
  process.stderr.write(
    "Verification stopped with unconfirmed inputs or evidence; no automatic replay.\n",
  );
  process.exitCode = 2;
}
