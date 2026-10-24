import { execFileSync } from "node:child_process";
import { lstat, readFile, realpath } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { userInfo } from "node:os";
import { resolve } from "node:path";
import { declarationSchema, type Declaration } from "./declaration.ts";
import { decode, encode } from "./exact-json.ts";
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
  const declaration = declarationSchema.parse(decode(await readFile(path, "utf8")));
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
  // Discovery uses the current product and harness with inherited test/Python
  // work left in place. Those files are not imported by this driver or built
  // into its normal executables. Confirmation still requires the clean tree.
  const discoveryExclusions =
    declaration.purpose === "discovery"
      ? [":(exclude)**/tests/**", ":(exclude)scripts/verification/*.py"]
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
  ]);
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
    } catch (error: unknown) {
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
      break;
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
    } catch {
      setupUnconfirmed = true;
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
    cleanupFailures,
    setupUnconfirmed,
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
