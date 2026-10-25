import { lstat, readFile } from "node:fs/promises";
import { userInfo } from "node:os";
import { resolve } from "node:path";
import { importDeclarationSchema, type ImportDeclaration } from "./import-declaration.ts";
import { encode } from "./exact-json.ts";
import { SealedLegacyCopy } from "./inputs/sealed-legacy-copy.ts";
import { Installation } from "./machines/installation.ts";
import { readLegacyOracle } from "./clients/legacy-oracle.ts";
import { readLegacyReaderIdentities } from "./inputs/legacy-reader-identities.ts";
import { compareLegacyInbox } from "./clients/import-inbox.ts";
import { verifyMacImportClaim } from "./clients/import-claim.ts";
import { runUpgrade, type UploadedUpgrade } from "./clients/board-upgrade.ts";
import { verifyImportReplay } from "./clients/import-replay.ts";
import { verifyCentralImport, verifyMacImport } from "./clients/import-readback.ts";
import { ObservationOpen, Receipt } from "./receipt.ts";
import type { CaseOutcome } from "./coverage.ts";
import { RunLifetime } from "./run-lifetime.ts";
import { enroll } from "./run-infrastructure.ts";
import { checkSource } from "./source-identity.ts";
import { TokioCentral } from "./targets/tokio-central.ts";

async function declarationFromArguments(): Promise<ImportDeclaration> {
  const path = process.argv[2];
  if (path === undefined || process.argv.length !== 3)
    throw new Error("usage: node verification/import-run.ts PRIVATE_MAC_IMPORT_DECLARATION.json");
  const metadata = await lstat(path);
  if (
    !metadata.isFile() ||
    metadata.isSymbolicLink() ||
    metadata.uid !== process.getuid?.() ||
    (metadata.mode & 0o077) !== 0 ||
    metadata.size > 1024 * 1024
  )
    throw new Error("bounded owner-private declaration required");
  const declaration = importDeclarationSchema.parse(JSON.parse(await readFile(path, "utf8")));
  const parent = `${resolve(userInfo().homedir, ".cache")}/`;
  for (const path of [
    declaration.evidenceDirectory,
    declaration.machine.root,
    declaration.machine.temporaryParent,
    declaration.target.root,
    declaration.target.assertionFile,
  ])
    if (resolve(path) !== path || !path.startsWith(parent))
      throw new Error("Mac-only import requires canonical private cache roots");
  return declaration;
}
async function publishWithReplay(input: {
  declaration: ImportDeclaration;
  central: TokioCentral;
  installation: Installation;
  copies: readonly SealedLegacyCopy[];
  receipt: Receipt;
}): Promise<UploadedUpgrade> {
  const { declaration, central, installation, copies, receipt } = input;
  const upgradeInput = { artifact: declaration.importer, installation, copies, receipt };
  const publication = {
    kind: "upload",
    centralURL: central.origin,
    authentication: { kind: "fixture", assertionFile: declaration.target.assertionFile },
  } satisfies Parameters<typeof runUpgrade>[0]["publication"];
  const dry = await runUpgrade({ ...upgradeInput, publication: { kind: "dryRun" } });
  if (dry.kind !== "dryRun") throw new Error("sealed-copy dry run refused");
  const imported = await runUpgrade({ ...upgradeInput, publication });
  if (imported.kind !== "uploaded") throw new Error("sealed-copy publication refused");
  await receipt.equal(
    "dry run and publication preserve Fleet content",
    imported.report.fleetDigest,
    dry.report.fleetDigest,
  );
  await receipt.equal(
    "declared central readback inventory matches imported projects",
    [...declaration.target.projects].sort(),
    imported.report.projects.map((project) => project.project).sort(),
  );
  const before = await verifyCentralImport({
    central,
    report: imported.report,
    receipt,
    phase: "initial",
  });
  const replayed = await runUpgrade({ ...upgradeInput, publication });
  if (replayed.kind !== "uploaded") throw new Error("identical sealed-copy rerun refused");
  await verifyImportReplay({ initial: imported, replayed, receipt });
  const after = await verifyCentralImport({
    central,
    report: imported.report,
    receipt,
    phase: "replayed",
  });
  await receipt.equal("identical import rerun changes no owning central state", after, before);
  await receipt.record("identicalImportReplay", {
    projects: imported.report.projects.length,
    fleetDigest: imported.report.fleetDigest,
    owningStateUnchanged: true,
  });
  return imported;
}
async function observeImport(
  declaration: ImportDeclaration,
  receipt: Receipt,
  lifetime: RunLifetime,
): Promise<void> {
  const copies: SealedLegacyCopy[] = [];
  const selections: {
    oracle: Awaited<ReturnType<typeof readLegacyOracle>>;
    readers: Awaited<ReturnType<typeof readLegacyReaderIdentities>>;
  }[] = [];
  await checkSource(declaration, receipt);
  lifetime.own("sealed-inputs", {
    close: async (): Promise<void> => {
      for (const copy of copies) await copy.verify();
    },
  });
  for (const source of declaration.legacyStores) {
    const copy = await SealedLegacyCopy.qualify(source);
    copies.push(copy);
    selections.push({
      oracle: await readLegacyOracle({ artifact: source.oracle, copy, receipt }),
      readers: await readLegacyReaderIdentities(copy),
    });
  }
  const central = lifetime.own(
    "central",
    await TokioCentral.start({
      target: declaration.target,
      repository: declaration.repository,
      receipt,
    }),
  );
  const installation = lifetime.own(
    "installation-mac",
    await Installation.prepare({
      machine: declaration.machine,
      repository: declaration.repository,
      productVersion: declaration.productVersion,
      receipt,
    }),
  );
  await enroll({
    installation,
    declaration,
    central,
    receipt,
    lifetime,
    gates: new Map(),
    links: [],
  });
  const imported = await publishWithReplay({ declaration, central, installation, copies, receipt });
  lifetime.afterDependencies(installation);
  await installation.start();
  await verifyMacImport({ installation, report: imported.report, receipt });
  for (const selection of selections)
    await compareLegacyInbox({
      ...selection,
      client: installation.mcp,
      receipt,
      deadline: declaration.deadline,
    });
  const claimed = await verifyMacImportClaim({
    installation,
    central,
    projects: imported.report.projects.filter((project) =>
      selections.some((selection) =>
        selection.oracle.selections.some(
          (row) => row.legacy_topic === project.project && row.post_count > 0n,
        ),
      ),
    ),
    receipt,
    deadline: declaration.deadline,
  });
  for (const selection of selections) {
    const claimedSelections = selection.oracle.selections.filter(
      (row) => row.legacy_topic === claimed.project,
    );
    if (claimedSelections.length === 0) continue;
    await compareLegacyInbox({
      ...selection,
      oracle: { ...selection.oracle, selections: claimedSelections },
      client: installation.mcp,
      receipt,
      deadline: declaration.deadline,
    });
  }
  await installation.check();
  await checkSource(declaration, receipt);
}
async function main(): Promise<void> {
  const declaration = await declarationFromArguments();
  const receipt = new Receipt(declaration.evidenceDirectory);
  await receipt.begin(declaration);
  const lifetime = new RunLifetime(receipt);
  let observation: CaseOutcome = { kind: "open", reason: "Mac import has not been observed" };
  let cleanupFailures: readonly string[] = [];
  try {
    await observeImport(declaration, receipt, lifetime);
    observation = { kind: "observed" };
  } catch (error: unknown) {
    observation =
      error instanceof ObservationOpen
        ? { kind: "open", reason: error.message }
        : { kind: "failed", reason: "Mac import action or readback failed" };
    await receipt.save(
      "import-failure.json",
      error instanceof Error
        ? {
            name: error.name,
            message: error.message,
            stack: error.stack,
            cause: error.cause,
          }
        : { kind: "nonErrorThrown" },
    );
  } finally {
    cleanupFailures = await lifetime.close();
  }
  const exit =
    observation.kind === "failed"
      ? 1
      : observation.kind === "observed" && cleanupFailures.length === 0
        ? 0
        : 2;
  const result = {
    purpose: declaration.purpose,
    dataBoundary: declaration.dataBoundary,
    releaseProofEligible: false,
    observation,
    agentTurnsStarted: 0,
    remoteInstallations: 0,
    cleanupFailures,
    crossMachineRealCopies: {
      kind: "open",
      reason: "owner authorization: real board metadata to Sunclaw",
    },
    unverified: [
      "remaining OB5 interruption, collision, changed-content and competing-claim groups",
    ],
    exit,
  };
  await receipt.save("result.json", result);
  process.stdout.write(
    `${encode({ dataBoundary: result.dataBoundary, observation, cleanupComplete: cleanupFailures.length === 0, agentTurnsStarted: 0, remoteInstallations: 0, exit })}\n`,
  );
  process.exitCode = exit;
}
try {
  await main();
} catch {
  process.stderr.write("Mac-only import stopped with unconfirmed inputs; no automatic replay.\n");
  process.exitCode = 2;
}
