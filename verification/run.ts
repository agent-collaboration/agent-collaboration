import { checkSource } from "./source-identity.ts";
import { lstat, readFile } from "node:fs/promises";
import { userInfo } from "node:os";
import { resolve } from "node:path";
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
import { assessD1Coverage, d1Observations } from "./observations/central/d1-coverage.ts";
import { runD1Harness } from "./observations/central/d1-harness.ts";
import { readD1HarnessDeclaration } from "./observations/central/d1-harness-plan.ts";
import { d1ReadbackSchema, readD1Stores } from "./observations/central/d1-sqlite-readback.ts";
import { readCompatibilityPlan } from "./observations/central/d3-wire-compatibility/compatibility-plan.ts";
import { runCompatibility } from "./observations/central/d3-wire-compatibility/compatibility-run.ts";
import { readD4OfflineHarnessPlan } from "./observations/central/d4-central-engine/offline-harness-plan.ts";
import { runOfflineCustody } from "./observations/central/d4-central-engine/offline-harness.ts";

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
function printPreparation(): boolean {
  if (process.argv.length === 3 && process.argv[2] === "--help") {
    process.stdout.write(
      "node verification/run.ts PRIVATE_DECLARATION.json\n" +
        "Run the existing declared cross-machine cases.\n" +
        "node verification/run.ts --d1-plan\n" +
        "Print D1 O1–O11 requirements and all-open coverage. No launch, network, credentials or writes.\n" +
        "node verification/run.ts --d1-harness PRIVATE_DECLARATION.json\n" +
        "Drive three isolated native-backed Hosts and selected no-model fixtures on local Tokio.\n" +
        "node verification/run.ts --d1-readback PRIVATE_DECLARATION.json\n" +
        "Read exact stopped task stores after consumer checks.\n" +
        "node verification/run.ts --d3-wire PRIVATE_DECLARATION.json\n" +
        "Local released 0.1.134 source/debug-build compatibility against declared Tokio central; no Keychain or model input.\n" +
        "node verification/run.ts --d4-offline-custody PRIVATE_DECLARATION.json\n" +
        "Declared immutable local Tokio/real-Router ten-item custody; Codex0.166.0 archive/unarchive, no model input. Launch from the compiled source archive root.\n" +
        "D1 full outcome credit requires declared controls/target; staging is never executed here.\n",
    );
    return true;
  }
  if (process.argv.length === 3 && process.argv[2] === "--d1-plan") {
    process.stdout.write(
      `${encode({
        suite: "d1-central-layout-and-mail",
        verificationDesign:
          "docs/specs/2026-11-02-central-storage-reset/verification-design/d1-central-layout-and-mail.md",
        executed: false,
        productProofEligible: false,
        observations: d1Observations,
        outcomes: Object.fromEntries(assessD1Coverage({ backend: "tokio", checks: new Map() })),
      })}\n`,
    );
    return true;
  }
  return false;
}
async function runD1Action(): Promise<boolean> {
  if (process.argv.length === 4 && process.argv[2] === "--d4-offline-custody") {
    const path = process.argv[3];
    if (path === undefined) throw new Error("D4 offline declaration absent");
    await runOfflineCustody(await readD4OfflineHarnessPlan(path));
    return true;
  }
  if (process.argv.length === 4 && process.argv[2] === "--d3-wire") {
    const path = process.argv[3];
    if (path === undefined) throw new Error("D3 plan absent");
    await runCompatibility(await readCompatibilityPlan(path));
    return true;
  }
  if (process.argv.length === 4 && process.argv[2] === "--d1-readback") {
    const path = process.argv[3];
    if (path === undefined) throw new Error("D1 readback declaration absent");
    await readD1Stores(d1ReadbackSchema.parse(JSON.parse(await readFile(path, "utf8"))));
    return true;
  }
  if (process.argv.length === 4 && process.argv[2] === "--d1-harness") {
    const path = process.argv[3];
    if (path === undefined) throw new Error("D1 declaration path absent");
    await runD1Harness(await readD1HarnessDeclaration(path));
    return true;
  }
  return false;
}
async function main(): Promise<void> {
  if (await runD1Action()) return;
  if (printPreparation()) return;
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
  let context: PreparedRun | undefined;
  if (missing.length === 0) {
    try {
      await checkSource(declaration, receipt);
      context = await prepareRun({ declaration, receipt, lifetime });
      await executeCases({ declaration, cases, context, receipt, results });
      await checkSource(declaration, receipt);
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
  const configurationEvidence =
    context?.machines.map((machine) => ({
      machine: machine.installation.machine.name,
      ...machine.installation.configurationEligibility(),
    })) ?? [];
  const configurationEligible =
    configurationEvidence.length === 2 &&
    configurationEvidence.every((observation) => observation.eligible);
  let exit = exitCode(outcomes);
  if (exit === 0 && (setupUnconfirmed || cleanupFailures.length > 0 || !configurationEligible))
    exit = 2;
  const releaseProofEligible =
    declaration.purpose === "confirmation" && exit === 0 && configurationEligible;
  await receipt.save("result.json", {
    purpose: declaration.purpose,
    releaseProofEligible,
    selected: declaration.rows,
    outcomes: Object.fromEntries(outcomes),
    executedCases: results.size,
    caseResults: Object.fromEntries(results),
    cleanupFailures,
    setupUnconfirmed,
    configurationObservation: {
      evidence: "observations.jsonl",
      changedFilesStopCases: false,
      eligible: configurationEligible,
      installations: configurationEvidence,
      ownedWriteAttribution: configurationEvidence.some(
        (observation) => observation.ownedWriteAccessDuringChange,
      )
        ? "changedFileOpenForWriteByOwnedProcess"
        : "notEstablished",
      limit:
        "Owned open-file descriptors sampled; syscall writes not traced. Changes alone are notes.",
    },
    dataBoundary: declaration.dataBoundary,
    crossMachineRealCopies: {
      kind: "open",
      reason: "owner authorization: real board metadata to Sunclaw",
    },
    exit,
  });
  process.stdout.write(
    `${encode({
      purpose: declaration.purpose,
      releaseProofEligible,
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
