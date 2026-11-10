/** One dedicated imported-unheld retirement identity, with explicit completion evidence. */
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

export const dedicatedRetirementProject = "01a1c438-6b0d-7490-91d1-1f46a8852fdd";
const dedicatedBoard = "01a1c438-6b0d-7802-926f-ef51b2a6eef4";
type RetirementCli = Pick<SyntheticCli, "execute" | "save">;
const position = z.object({ epoch: z.bigint().nonnegative(), sequence: z.bigint().nonnegative() });
const retirementFacts = z.object({
  fence: z.bigint().positive(),
  acked: position,
});
const cliFailure = z.object({
  kind: z.literal("syntheticCliAnswer"),
  exit: z.number().int(),
  answer: z.object({ kind: z.string() }).passthrough(),
});
interface RetirementCompletion {
  readonly actualOwnerRetiredReply: boolean;
  readonly ownerInvocations: 1 | 2;
}

function stableDirectoryGrant(entry: z.infer<typeof oldDirectoryEntry>): unknown {
  const lease = entry.leaseSide.kind === "known" ? entry.leaseSide.value.lease : entry.leaseSide;
  return {
    project: entry.project,
    name: entry.name,
    workspace: entry.workspace,
    board: entry.board,
    boardArchive: entry.boardArchive,
    lease:
      lease.kind === "held"
        ? {
            kind: "held",
            epoch: lease.value.epoch,
            holder: lease.value.holder,
            state: lease.value.state,
          }
        : lease,
  };
}

export function validateRetirementFacts(input: {
  health: unknown;
  directory: unknown;
  projectReadback: unknown;
  board: unknown;
}): z.infer<typeof retirementFacts> {
  const health = healthReply.parse(input.health),
    entry = oldDirectoryEntry.parse(input.directory);
  z.object({
    record: z.object({
      project: z.object({
        id: z.literal(dedicatedRetirementProject),
        board: z.literal(dedicatedBoard),
      }),
    }),
  }).parse(input.projectReadback);
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
    entry.leaseSide.value.lease.kind !== "unheld"
  )
    throw new Error("dedicated archived project must be imported and unheld");
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
    copy.holder !== null
  )
    throw new Error("dedicated Copy must be authoritative and unheld; no retirement sent");
  return retirementFacts.parse({ fence: copy.fence, acked: copy.acked });
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
  cli: RetirementCli,
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

async function captureBefore(cli: SyntheticCli): Promise<{
  cohort: z.infer<typeof oldDirectoryEntry>[];
  facts: z.infer<typeof retirementFacts>;
}> {
  const project = dedicatedRetirementProject;
  const projectReadback = await cli.execute("project-before", [
    "project",
    "show",
    "--project-id",
    project,
  ]);
  const board = await waitForArchived(cli, dedicatedBoard);
  const cohort = await registryDirectory(cli, "before");
  const directory = cohort.find((entry): boolean => entry.project === project);
  const health = await cli.execute("health-before", ["central", "copy", "health", project], true);
  const facts = validateRetirementFacts({
    health,
    directory,
    projectReadback,
    board,
  });
  await cli.save("preconditions", {
    project,
    board: dedicatedBoard,
    holder: null,
    lifecycle: "unheld",
    fence: facts.fence,
    acked: facts.acked,
    source: "fresh authoritative Copy health; an unheld project has no holder takeover preview",
  });
  return { cohort, facts };
}

async function reconcileRetirement(
  cli: RetirementCli,
  input: {
    before: z.infer<typeof oldDirectoryEntry>[];
    requestId: string;
    ownerReply: unknown;
    requestArguments: readonly string[];
  },
): Promise<RetirementCompletion> {
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
    for (const original of input.before.filter(
      (entry) => entry.project !== dedicatedRetirementProject,
    )) {
      const current = cohort.find((entry) => entry.project === original.project);
      if (
        current === undefined ||
        encode(stableDirectoryGrant(original)) !== encode(stableDirectoryGrant(current))
      )
        throw new Error("another project's directory or stable grant changed across retirement");
    }
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
      let ownerReply = input.ownerReply;
      let ownerInvocations: 1 | 2 = 1;
      const pending = z
        .object({
          kind: z.literal("pending"),
          value: z.object({ requestId: z.literal(input.requestId) }),
        })
        .safeParse(ownerReply);
      if (pending.success) {
        // The API's known-pending completion uses the SAME saved identity/body.
        ownerReply = await cli.execute(
          "retire-terminal-same-request",
          input.requestArguments,
          true,
        );
        ownerInvocations = 2;
      }
      const actualOwnerRetiredReply = z
        .object({ kind: z.literal("retired") })
        .safeParse(ownerReply).success;
      await cli.save("retirement-public-state-fact", {
        project: dedicatedRetirementProject,
        requestId: input.requestId,
        ownerReply,
        actualOwnerRetiredReply,
        ownerInvocations,
        copy: rawHealth,
        registry: entry,
        membership: expectedMembership,
        otherDirectoryGrantsUnchanged: true,
      });
      return { actualOwnerRetiredReply, ownerInvocations };
    }
    if (sample < 30) await setTimeout(30_000);
  }
  throw new Error("retirement public state remains incomplete; the retire request was not resent");
}

export async function retireOnce(
  cli: RetirementCli,
  before: Awaited<ReturnType<typeof captureBefore>>,
): Promise<RetirementCompletion> {
  const requestId = randomUUIDv7();
  const request = {
    project: dedicatedRetirementProject,
    requestId,
    expectFence: before.facts.fence,
    expectAcked: before.facts.acked,
  };
  const requestArguments = [
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
  ];
  await cli.save("retirement-request", request);
  let answer: unknown;
  try {
    answer = await cli.execute("retire-once", requestArguments, true);
    const kind = z.object({ kind: z.enum(["pending", "retired"]) }).parse(answer).kind;
    if (kind === "pending")
      z.object({ value: z.object({ requestId: z.literal(requestId) }) }).parse(answer);
  } catch (error: unknown) {
    const explicit = error instanceof Error ? cliFailure.safeParse(error.cause) : undefined;
    if (
      explicit?.success &&
      explicit.data.exit === 4 &&
      ["refused", "notFound", "conflictingRequest"].includes(explicit.data.answer.kind)
    ) {
      await cli.save("retirement-explicit-refusal", {
        requestId,
        answer: explicit.data.answer,
        sentOnce: true,
      });
      throw new Error("retirement explicitly refused; no reconciliation or resend", {
        cause: error,
      });
    }
    if (explicit?.success && explicit.data.exit === 3) {
      await cli.save("retirement-known-unavailable", {
        requestId,
        answer: explicit.data.answer,
        sentOnce: true,
      });
      throw new Error("retirement unavailable; no automatic polling or resend", { cause: error });
    }
    await cli.save("retirement-outcome-unknown", {
      requestId,
      sentAtMostOnce: true,
      message: error instanceof Error ? error.message : String(error),
    });
    await reconcileRetirement(cli, {
      before: before.cohort,
      requestId,
      ownerReply: { kind: "outcomeUnknown" },
      requestArguments,
    });
    throw new Error("retirement answer unknown; public state reconciled and no resend performed");
  }
  await cli.save("retirement-owner-reply", {
    project: dedicatedRetirementProject,
    requestId,
    answer,
    actualOwnerRetiredReply: z.object({ kind: z.literal("retired") }).safeParse(answer).success,
  });
  return reconcileRetirement(cli, {
    before: before.cohort,
    requestId,
    ownerReply: answer,
    requestArguments,
  });
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
  await cli.save(`unheld-target-${phase}`, {
    project: dedicatedRetirementProject,
    nativeWriterNotClaimedByThisCase: true,
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
  for (const name of existing.filter(
    (name) =>
      name.endsWith("retirement-request.json") || name.endsWith("-retire-once-request.json"),
  )) {
    const prior = z
      .object({ project: z.string().optional(), arguments: z.array(z.string()).optional() })
      .parse(decodeAuthoredJson(await readPrivateText(`${syntheticReceipts}/${name}`)));
    if (
      prior.project === dedicatedRetirementProject ||
      (prior.arguments?.[0] === "central" &&
        prior.arguments[1] === "project" &&
        prior.arguments[2] === "retire" &&
        prior.arguments[3] === dedicatedRetirementProject)
    )
      throw new Error(
        "a saved retirement invocation exists for this project; reconcile without another send",
      );
  }
  const saved = z
    .object({ board: z.literal(dedicatedBoard), project: z.literal(dedicatedRetirementProject) })
    .parse(
      decodeAuthoredJson(
        await readPrivateText(`${syntheticReceipts}/retirement-unheld/manifest.json`),
      ),
    );
  const cli = await SyntheticCli.open("retirementContinuation", attempt, executionOverlay);
  try {
    await cli.save("commission", {
      leadTrailRow: "01a1c491-8858-7d26-94e8-9827c60d2304",
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
    const completed = await retireOnce(cli, before);
    if (!completed.actualOwnerRetiredReply)
      throw new Error(
        "public retirement is visible, but the owner's terminal Retired answer remains unobserved",
      );
    // A retired Copy intentionally refuses ProjectState reads. The retained
    // Directory and minimal Copy receipt above are this case's after facts.
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
      retireInvocations: completed.ownerInvocations,
      retirementRequestIds: 1,
      actualOwnerRetiredReply: true,
      hostAndEnrollmentUnchanged: true,
      otherDirectoryGrantsUnchanged: true,
    });
  } catch (error: unknown) {
    await cli.save("incomplete", {
      message: error instanceof Error ? error.message : String(error),
      noNewRetirementIntentOnFailure: true,
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
