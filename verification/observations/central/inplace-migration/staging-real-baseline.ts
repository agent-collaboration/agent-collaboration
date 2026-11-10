/** Open only the declared sealed-copy cohort on an already released isolated staging target. */
import { mkdir, writeFile, lstat } from "node:fs/promises";
import { createInterface } from "node:readline";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { artifactSchema, localMachineSchema } from "../../../declaration.ts";
import { decode, decodeAuthoredJson, encode } from "../../../exact-json.ts";
import { Installation } from "../../../machines/installation.ts";
import { prepareStagingCodexHome } from "../../../machines/codex-home.ts";
import { prepareStagingUserHome } from "../../../machines/user-home-isolation.ts";
import { command, privateDirectory } from "../../../machines/owned-process.ts";
import { Receipt } from "../../../receipt.ts";
import type { CliOutcome } from "../../../clients/router-cli.ts";
import { backupDatabase, hashFile } from "./project-backups.ts";
import { readPrivateText } from "./record.ts";

const original = z
  .object({
    project: z.uuidv7(),
    sourceRelative: z.string(),
    untouched: z.string().startsWith("/"),
    sha256: z.string().regex(/^[a-f0-9]{64}$/),
    holder: z.uuidv7(),
    lease: z.string().regex(/^[1-9][0-9]*$/),
    originEpoch: z.string().regex(/^[1-9][0-9]*$/),
  })
  .strict();
const planSchema = z
  .object({
    machine: localMachineSchema,
    stagingHome: z.string(),
    repository: z.string(),
    sourceHead: z.string().regex(/^[a-f0-9]{40}$/),
    productVersion: z.literal("0.1.125"),
    managedCodex: artifactSchema,
    installationId: z.uuid(),
    peerId: z.uuidv7(),
    origin: z.literal("https://s1-migration.agent-collaboration.com"),
    serviceTokenFile: z.string(),
    /** A Mac without the owner's browser sign-in reads an assertion minted on another Mac. */
    ownerAssertionFile: z.string().startsWith("/").optional(),
    evidenceDirectory: z.string(),
    expectedFiles: z.number().int().positive(),
    files: z.array(original).min(1),
  })
  .strict();
type BaselinePlan = z.infer<typeof planSchema>;
async function checkFiles(plan: BaselinePlan): Promise<void> {
  if (
    !plan.machine.temporaryParent.startsWith("/private/tmp/2026-11-s1-") ||
    plan.files.length !== plan.expectedFiles ||
    new Set(plan.files.map((entry): string => entry.project)).size !== plan.expectedFiles
  )
    throw new Error("exact private staging cohort required");
  for (const file of plan.files) {
    const expected = `${file.project}/e${file.originEpoch}.sqlite`;
    if (
      file.sourceRelative !== expected ||
      file.originEpoch !== file.lease ||
      file.holder !== plan.peerId
    )
      throw new Error("original filename/origin/holder/lease placement mismatch");
    const info = await lstat(file.untouched);
    if (
      !info.isFile() ||
      info.isSymbolicLink() ||
      info.uid !== process.getuid?.() ||
      (info.mode & 0o777) !== 0o400 ||
      (await hashFile(file.untouched)) !== file.sha256
    )
      throw new Error("sealed original B identity/hash differs before staging preparation");
  }
}
async function prepare(
  plan: BaselinePlan,
): Promise<{ installation: Installation; receipt: Receipt }> {
  await checkFiles(plan);
  await mkdir(plan.machine.temporaryParent, { mode: 0o700 });
  await mkdir(plan.machine.nativeCwd, { mode: 0o700 });
  await mkdir(plan.evidenceDirectory, { mode: 0o700 });
  const receipt = new Receipt(plan.evidenceDirectory);
  await receipt.save("baseline-plan.json", plan);
  await prepareStagingUserHome({ machine: plan.machine, home: plan.stagingHome });
  await prepareStagingCodexHome({ machine: plan.machine, managedCodex: plan.managedCodex });
  const installation = await Installation.prepare({
    machine: plan.machine,
    repository: plan.repository,
    productVersion: plan.productVersion,
    receipt,
    codexHomePurpose: "staging-isolated",
    stagingHome: plan.stagingHome,
  });
  const collaboration = `${plan.machine.root}/agent-communication`;
  await mkdir(collaboration, { mode: 0o700 });
  await writeFile(
    `${collaboration}/service-identity.json`,
    encode({ version: 1, serviceId: plan.installationId }),
    { mode: 0o600, flag: "wx" },
  );
  await writeFile(
    `${collaboration}/installation-peer.json`,
    encode({ version: 1, peerId: plan.peerId }),
    { mode: 0o600, flag: "wx" },
  );
  const placements = [];
  for (const file of plan.files) {
    const directory = `${collaboration}/projects/${file.project}`;
    await mkdir(directory, { recursive: true, mode: 0o700 });
    await privateDirectory(directory);
    const path = `${collaboration}/projects/${file.sourceRelative}`;
    const backup = await backupDatabase({
      source: file.untouched,
      destination: path,
      sealedSource: true,
    });
    if ((await hashFile(file.untouched)) !== file.sha256)
      throw new Error("sealed B changed during runtime-copy placement");
    placements.push({
      project: file.project,
      path,
      originEpoch: file.originEpoch,
      holder: file.holder,
      lease: file.lease,
      originalB: { path: file.untouched, sha256: file.sha256 },
      ...backup,
    });
  }
  await receipt.save("placements.json", placements);
  return { installation, receipt };
}
async function enroll(
  plan: BaselinePlan,
  installation: Installation,
  receipt: Receipt,
): Promise<void> {
  const token = z
    .object({
      client_id: z.string().min(1),
      client_secret: z.string().min(1),
      expires_at: z.iso.datetime(),
    })
    .parse(decodeAuthoredJson(await readPrivateText(plan.serviceTokenFile)));
  const inputs = `${plan.machine.root}/verification-inputs`;
  await mkdir(inputs, { mode: 0o700 });
  const secretInput = `${inputs}/access-client-secret`;
  await writeFile(secretInput, token.client_secret, { mode: 0o600, flag: "wx" });
  const assertion = await ownerAssertion(plan);
  if (assertion === "")
    throw new Error("owner Access assertion unavailable; no enrollment attempted");
  const invokedAt = new Date().toISOString();
  const result = await installation.runRouterCommand(
    [
      "fleet",
      "enroll",
      "--router-root",
      plan.machine.root,
      "--endpoint",
      `${plan.origin}/v1/session`,
      "--label",
      `S1 ${plan.machine.name} isolated real-copy baseline`,
      "--access-client-id",
      token.client_id,
      "--access-client-secret-file",
      secretInput,
      "--expires-at",
      token.expires_at,
      "--owner-token-env",
      "STAGING_BASELINE_OWNER_ASSERTION",
      "--allow-plaintext-file-secrets",
    ],
    { STAGING_BASELINE_OWNER_ASSERTION: assertion },
  );
  await receipt.save("enrollment-receipt.json", { invokedAt, ...result });
  if (result.exit !== 0)
    throw new Error("staging enrollment outcome requires reconciliation; do not rerun");
  const accepted = z
    .object({
      decision: z.object({
        kind: z.literal("machineEnrolled"),
        value: z.object({ peer: z.literal(plan.peerId) }),
      }),
    })
    .parse(decode(result.stdout));
  if (accepted.decision.kind !== "machineEnrolled") throw new Error("staging enrollment refused");
}
async function ownerAssertion(plan: BaselinePlan): Promise<string> {
  if (plan.ownerAssertionFile !== undefined)
    return (await readPrivateText(plan.ownerAssertionFile)).trim();
  const access = await command("cloudflared", ["access", "token", `--app=${plan.origin}`], {
    timeoutSeconds: 60,
  });
  return access.exit === 0 ? access.stdout.trim() : "";
}
/** Seconds a freshly started old125 Host may take to cache the staging Fleet directory. */
const DIRECTORY_SYNC_DEADLINE_SECONDS = 180;
/**
 * Old125 `project show` resolves the Fleet directory entry before opening the local file
 * (collaboration-service board_read_operations.rs `project_show`), so a read issued before the
 * new Host caches the staging directory answers `resourceNotFound`. Retry only that refusal.
 */
async function showOnceDirectoryListed(
  installation: Installation,
  project: string,
): Promise<{ outcome: CliOutcome; attempts: number; retried: CliOutcome[] }> {
  const deadline = Date.now() + DIRECTORY_SYNC_DEADLINE_SECONDS * 1000;
  // Keep each retried refusal body so its resourceNotFound classification stays auditable.
  const retried: CliOutcome[] = [];
  for (let attempts = 1; ; attempts += 1) {
    const outcome = await installation.cli.outcome(["project", "show", "--project-id", project]);
    const notYetListed =
      outcome.kind === "refused" &&
      z.object({ kind: z.literal("resourceNotFound") }).safeParse(outcome.value).success;
    if (!notYetListed || Date.now() >= deadline) return { outcome, attempts, retried };
    retried.push(outcome);
    await setTimeout(2000);
  }
}
async function run(plan: BaselinePlan): Promise<void> {
  process.umask(0o077);
  const { installation, receipt } = await prepare(plan);
  const controls = createInterface({ input: process.stdin, crlfDelay: Infinity });
  let active = true,
    failure: unknown,
    baselineReady = false;
  const close = (): void => {
    controls.close();
  };
  process.once("SIGTERM", close);
  process.once("SIGINT", close);
  let monitor = Promise.resolve();
  // The baseline mark refuses while any bounded command is running, and each isolation check
  // runs bounded ps/lsof samples; both take this turn so a sample never straddles the mark.
  let ledgerTurn: Promise<unknown> = Promise.resolve();
  const exclusive = <Value>(operation: () => Promise<Value>): Promise<Value> => {
    const result = ledgerTurn.then(operation);
    ledgerTurn = result.catch((): void => {});
    return result;
  };
  try {
    await enroll(plan, installation, receipt);
    const openedAt = new Date().toISOString();
    await receipt.save("host-window.json", { openedAt, expectedFiles: plan.expectedFiles });
    await installation.start();
    monitor = (async (): Promise<void> => {
      while (active) {
        try {
          await exclusive(() => installation.check());
        } catch (error: unknown) {
          failure = error;
          controls.close();
          return;
        }
        await setTimeout(500);
      }
    })();
    const reads = [];
    for (const file of plan.files) {
      const { outcome, attempts, retried } = await showOnceDirectoryListed(
        installation,
        file.project,
      );
      await receipt.save(`project-${file.project}.json`, { attempts, retried, ...outcome });
      if (outcome.kind !== "answered")
        throw new Error(
          `ordinary old125 project open/read refused or unconfirmed: ${file.project}`,
        );
      z.object({
        record: z.object({
          state: z.object({
            kind: z.literal("local"),
            value: z.object({ projectId: z.literal(file.project) }),
          }),
        }),
      }).parse(outcome.value);
      reads.push(file.project);
    }
    await receipt.save("old125-read-summary.json", {
      openedAt,
      projects: reads,
      count: reads.length,
      currentRenewalObserved: false,
    });
    const baseline = await exclusive(() => installation.markUpgradeBaseline());
    await receipt.save("baseline-for-upgrade.json", baseline);
    baselineReady = true;
    process.stdout.write(
      `${encode({ kind: "old125CopiesReadable", files: reads.length, currentRenewalObserved: false, root: plan.machine.root })}\n`,
    );
    for await (const line of controls) {
      const request = z
        .object({ kind: z.literal("stop") })
        .strict()
        .parse(decode(line));
      if (request.kind === "stop") break;
    }
  } catch (error: unknown) {
    await receipt.save("baseline-failure.json", {
      baselineReady,
      message: error instanceof Error ? error.message : String(error),
    });
    throw error;
  } finally {
    active = false;
    controls.close();
    process.removeListener("SIGTERM", close);
    process.removeListener("SIGINT", close);
    await monitor;
    await installation.close();
  }
  if (failure !== undefined) throw failure;
}
if (import.meta.main) {
  const path = process.argv[2];
  if (path === undefined || process.argv.length !== 3) process.exitCode = 3;
  else
    void readPrivateText(path)
      .then((text): BaselinePlan => planSchema.parse(decodeAuthoredJson(text)))
      .then(run)
      .catch((error: unknown): void => {
        process.stderr.write(
          `${encode({ kind: "old125BaselineIncomplete", message: error instanceof Error ? error.message : String(error) })}\n`,
        );
        process.exitCode = 1;
      });
}
