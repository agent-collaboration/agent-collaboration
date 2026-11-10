/** Lead-released separate synthetic board/project; one retirement send, then public reads only. */
import { randomUUIDv7 } from "node:crypto";
import { readdir } from "node:fs/promises";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { decodeAuthoredJson, encode } from "../../../exact-json.ts";
import { readPrivateText } from "./record.ts";
import { SyntheticCli, syntheticReceipts, stagingRoot } from "./s1-synthetic-cli.ts";
import {
  directoryEntry as oldDirectoryEntry,
  directoryPage as oldDirectoryPage,
} from "./contracts.ts";
import { healthReply } from "./contracts.ts";
import { hashFile } from "./project-backups.ts";
import { loadOwnedHostObservation, readOwnedHostProcess } from "./s1-owned-host-observation.ts";

export const dedicatedRetirementProject = "01a1c307-6ffd-7b12-bdbd-4ec6ad1030b3";
const dedicatedBoard = "01a1c307-6ffd-716c-9957-0b382a7018e4";
const position = z.object({ epoch: z.bigint().nonnegative(), sequence: z.bigint().nonnegative() });
const previewReply = z.object({
  preview: z.object({
    token: z.string().min(1),
    holder: z.uuidv7(),
    fence: z.bigint().positive(),
    acked: position,
  }),
  holderInstallation: z.uuid(),
});

export function validateRetirementFacts(input: {
  preview: unknown;
  health: unknown;
  directory: unknown;
  local: unknown;
  board: unknown;
  installation: string;
  peer: string;
}): z.infer<typeof previewReply> {
  const preview = previewReply.parse(input.preview),
    health = healthReply.parse(input.health),
    entry = oldDirectoryEntry.parse(input.directory);
  z.object({
    record: z.object({
      project: z.object({
        id: z.literal(dedicatedRetirementProject),
        board: z.literal(dedicatedBoard),
      }),
    }),
  }).parse(input.local);
  z.object({
    record: z.object({
      board: z.object({ boardId: z.literal(dedicatedBoard), state: z.literal("archived") }),
    }),
  }).parse(input.board);
  if (
    entry.project !== dedicatedRetirementProject ||
    entry.board !== dedicatedBoard ||
    entry.boardArchive !== "archived" ||
    entry.leaseSide.kind !== "known" ||
    entry.leaseSide.value.lease.kind !== "held"
  )
    throw new Error("dedicated archived project has no current Registry grant");
  const lease = entry.leaseSide.value.lease.value;
  if (
    health.kind !== "live" ||
    health.value.copy.kind !== "known" ||
    health.value.copy.value === null
  )
    throw new Error("dedicated Copy facts unavailable");
  const copy = health.value.copy.value;
  if (
    copy.database !== dedicatedRetirementProject ||
    copy.role !== "authoritative" ||
    copy.holder !== input.peer ||
    preview.preview.holder !== input.peer ||
    preview.holderInstallation !== input.installation ||
    lease.holder !== input.installation ||
    lease.epoch !== copy.fence ||
    preview.preview.fence !== copy.fence ||
    encode(preview.preview.acked) !== encode(copy.acked)
  )
    throw new Error("fresh preview/Registry/Copy D/M/N or ACK differs; no retirement sent");
  return preview;
}

async function waitForArchived(cli: SyntheticCli, board: string): Promise<unknown> {
  for (let sample = 0; sample <= 30; sample += 1) {
    const reply = await cli.execute(`board-state-${sample}`, [
      "board",
      "show",
      "--board-id",
      board,
    ]);
    const state = z
      .object({
        record: z.object({
          board: z.object({ boardId: z.literal(board), state: z.enum(["archiving", "archived"]) }),
        }),
      })
      .parse(reply).record.board.state;
    process.stdout.write(`${encode({ case: "retirement-board", sample, state })}\n`);
    if (state === "archived") {
      await cli.save("archived-board-fact", reply);
      return reply;
    }
    if (sample < 30) await setTimeout(30_000);
  }
  throw new Error(
    "retirement board still Archiving after bounded public reads; no retirement sent",
  );
}
async function registryDirectory(
  cli: SyntheticCli,
  sample: string,
): Promise<z.infer<typeof oldDirectoryEntry>[]> {
  let after: string | null = null;
  const seen = new Set<string>();
  const projects: z.infer<typeof oldDirectoryEntry>[] = [];
  const projectIds = new Set<string>();
  for (let page = 0; page < 1000; page += 1) {
    const result = oldDirectoryPage.parse(
      await cli.execute(
        `directory-${sample}-${page}`,
        ["central", "copies", "--limit", "100", ...(after === null ? [] : ["--after", after])],
        true,
      ),
    );
    for (const entry of result.projects) {
      if (projectIds.has(entry.project)) throw new Error("duplicate Registry cohort member");
      projectIds.add(entry.project);
      projects.push(entry);
    }
    after = result.nextAfter;
    if (after === null) {
      await cli.save(`directory-${sample}-fact`, {
        projects,
        membership: [...projectIds].sort(),
        complete: true,
      });
      return projects;
    }
    if (seen.has(after) || result.projects.length === 0)
      throw new Error("directory paging did not advance");
    seen.add(after);
  }
  throw new Error("directory paging bound exceeded");
}

async function captureBefore(
  cli: SyntheticCli,
): Promise<{ cohort: z.infer<typeof oldDirectoryEntry>[]; preview: z.infer<typeof previewReply> }> {
  const project = dedicatedRetirementProject;
  const local = await cli.execute("local-before", ["project", "show", "--project-id", project]);
  const board = await waitForArchived(cli, dedicatedBoard);
  const cohort = await registryDirectory(cli, "before");
  const directory = cohort.find((entry): boolean => entry.project === project);
  // Own preview is obtained through this attempt's registered old125 CLI, never reused.
  const rawPreview = await cli.execute(
    "preview",
    ["central", "project", "takeover", "preview", project],
    true,
  );
  const health = await cli.execute("health-before", ["central", "copy", "health", project], true);
  const preview = validateRetirementFacts({
    preview: rawPreview,
    health,
    directory,
    local,
    board,
    installation: cli.baseline.installationId,
    peer: cli.baseline.peerId,
  });
  await cli.save("preconditions", {
    project,
    board: dedicatedBoard,
    installation: preview.holderInstallation,
    peer: preview.preview.holder,
    fence: preview.preview.fence,
    acked: preview.preview.acked,
    previewTokenRetainedOnlyInPrivateReply: true,
    source:
      "old125 central_commands.rs retirement accepts requestId/fence/ACK; the preview token has no retirement flag",
  });
  return { cohort, preview };
}

async function reconcileRetirement(
  cli: SyntheticCli,
  input: { before: z.infer<typeof oldDirectoryEntry>[]; requestId: string; ownerReply: unknown },
): Promise<void> {
  const expectedMembership = input.before.map((entry): string => entry.project).sort();
  for (let sample = 0; sample <= 30; sample += 1) {
    const rawHealth = await cli.execute(
      `health-after-${sample}`,
      ["central", "copy", "health", dedicatedRetirementProject],
      true,
    );
    const health = healthReply.parse(rawHealth);
    const cohort = await registryDirectory(cli, `after-${sample}`);
    if (encode(cohort.map((entry): string => entry.project).sort()) !== encode(expectedMembership))
      throw new Error("Registry cohort membership changed across retirement");
    const entry = cohort.find((item): boolean => item.project === dedicatedRetirementProject);
    if (entry === undefined) throw new Error("dedicated project missing from the preserved cohort");
    const copyRetired = health.kind === "retired",
      registryRetired =
        entry.leaseSide.kind === "known" && entry.leaseSide.value.lease.kind === "retired";
    await cli.save(`retirement-state-${sample}`, {
      project: dedicatedRetirementProject,
      requestId: input.requestId,
      ownerReply: input.ownerReply,
      copy: rawHealth,
      registry: entry,
      membershipCount: cohort.length,
      membershipPreserved: true,
    });
    process.stdout.write(
      `${encode({ case: "retirement", sample, copyRetired, registryRetired })}\n`,
    );
    if (copyRetired && registryRetired) {
      await cli.save("retirement-public-state-fact", {
        project: dedicatedRetirementProject,
        requestId: input.requestId,
        ownerReply: input.ownerReply,
        actualOwnerRetiredReply: z
          .object({ kind: z.literal("retired") })
          .safeParse(input.ownerReply).success,
        copy: rawHealth,
        registry: entry,
        membership: expectedMembership,
      });
      return;
    }
    if (sample < 30) await setTimeout(30_000);
  }
  throw new Error("retirement public state remains incomplete; the retire request was not resent");
}

async function retireOnce(
  cli: SyntheticCli,
  before: Awaited<ReturnType<typeof captureBefore>>,
): Promise<void> {
  const requestId = randomUUIDv7();
  const request = {
    project: dedicatedRetirementProject,
    requestId,
    expectFence: before.preview.preview.fence,
    expectAcked: before.preview.preview.acked,
  };
  await cli.save("retirement-request", request);
  let answer: unknown;
  try {
    answer = await cli.execute(
      "retire-once",
      [
        "central",
        "project",
        "retire",
        dedicatedRetirementProject,
        "--request-id",
        requestId,
        "--expect-fence",
        request.expectFence.toString(),
        "--expect-acked",
        encode(request.expectAcked),
      ],
      true,
    );
  } catch (error: unknown) {
    await cli.save("retirement-outcome-unknown", {
      requestId,
      sentAtMostOnce: true,
      message: error instanceof Error ? error.message : String(error),
    });
    await reconcileRetirement(cli, {
      before: before.cohort,
      requestId,
      ownerReply: { kind: "outcomeUnknown" },
    });
    throw new Error("retirement answer unknown; public state reconciled and no resend performed");
  }
  const kind = z.object({ kind: z.enum(["pending", "retired"]) }).parse(answer).kind;
  if (kind === "pending")
    z.object({ value: z.object({ requestId: z.literal(requestId) }) }).parse(answer);
  await cli.save("retirement-owner-reply", {
    project: dedicatedRetirementProject,
    requestId,
    answer,
    actualOwnerRetiredReply: kind === "retired",
  });
  await reconcileRetirement(cli, { before: before.cohort, requestId, ownerReply: answer });
}

async function observeLocal(
  cli: SyntheticCli,
  phase: "before" | "after",
  ownedHostFile: string,
): Promise<{ host: string; enrollment: string; selectionSha256: string }> {
  const ownedHost = await loadOwnedHostObservation(ownedHostFile);
  await cli.save(`owned-host-${phase}`, ownedHost);
  const observed = await readOwnedHostProcess(ownedHost);
  await cli.save(`host-${phase}`, observed);
  if (
    observed.exit !== 0 ||
    !observed.stdout.includes(stagingRoot) ||
    !observed.stdout.includes(" host ")
  )
    throw new Error("selected current owned Host is not observable");
  const enrollment = await hashFile(`${stagingRoot}/fleet/enrollment.json`);
  const localFiles = await readdir(
    `${stagingRoot}/agent-communication/projects/${dedicatedRetirementProject}`,
  );
  await cli.save(`local-files-${phase}`, {
    project: dedicatedRetirementProject,
    entries: localFiles.sort(),
    enrollmentSha256: enrollment,
  });
  return { host: observed.stdout, enrollment, selectionSha256: ownedHost.selectionSha256 };
}

async function run(): Promise<void> {
  process.umask(0o077);
  const [mode, attemptFlag, rawAttempt, overlayFlag, executionOverlay, hostFlag, ownedHostFile] =
    process.argv.slice(2);
  if (
    mode !== "--continue-after-archived" ||
    attemptFlag !== "--receipt-attempt" ||
    process.argv.length !== 9 ||
    overlayFlag !== "--execution-overlay" ||
    !executionOverlay?.startsWith("/") ||
    hostFlag !== "--owned-host-observation" ||
    !ownedHostFile?.startsWith("/")
  )
    throw new Error(
      "use --continue-after-archived --receipt-attempt SAVED_UUIDv7 for the dedicated project only",
    );
  const attempt = z.uuidv7().parse(rawAttempt);
  const existing = await readdir(syntheticReceipts, { recursive: true });
  if (
    existing.some(
      (name): boolean =>
        name.endsWith("retirement-request.json") || name.endsWith("-retire-once-request.json"),
    )
  )
    throw new Error(
      "a saved retirement invocation exists; reconcile existing state without another send",
    );
  const saved = z
    .object({ board: z.literal(dedicatedBoard), project: z.literal(dedicatedRetirementProject) })
    .parse(
      decodeAuthoredJson(await readPrivateText(`${syntheticReceipts}/retirement/manifest.json`)),
    );
  const cli = await SyntheticCli.open("retirementContinuation", attempt, executionOverlay);
  try {
    await cli.save("commission", {
      leadPush: "01a1c34d-3edf-70e0-a848-d08074cd0c51",
      target: saved,
      receiptAttempt: attempt,
      cli: cli.baseline.machine.cli,
      root: stagingRoot,
    });
    const selectedHost = await loadOwnedHostObservation(ownedHostFile);
    if ((await hashFile(executionOverlay)) !== selectedHost.selection.continuationPlan.sha256)
      throw new Error("retirement execution overlay differs from selected owned generation");
    const original = await observeLocal(cli, "before", ownedHostFile);
    const before = await captureBefore(cli);
    await retireOnce(cli, before);
    await cli.execute("local-after", ["project", "show", "--project-id", saved.project]);
    const after = await observeLocal(cli, "after", ownedHostFile);
    if (
      original.host !== after.host ||
      original.enrollment !== after.enrollment ||
      original.selectionSha256 !== after.selectionSha256
    )
      throw new Error("root-3 Host or enrollment identity changed");
    await cli.save("complete", {
      project: saved.project,
      board: saved.board,
      retireInvocations: 1,
      hostAndEnrollmentUnchanged: true,
      noOtherProjectMutation: true,
    });
  } catch (error: unknown) {
    await cli.save("incomplete", {
      message: error instanceof Error ? error.message : String(error),
      retireRequestNeverResent: true,
    });
    throw error;
  } finally {
    cli.close();
  }
}

if (import.meta.main)
  void run().catch((error: unknown): void => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
