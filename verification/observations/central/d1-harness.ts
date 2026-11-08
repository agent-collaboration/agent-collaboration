/** Three local native-backed Hosts against a real Tokio central. */
import { mkdir, readdir, realpath, lstat, readFile } from "node:fs/promises";
import { dirname } from "node:path";
import { z } from "zod";
import { encode, decode } from "../../exact-json.ts";
import { OwnedProcess, privateDirectory, qualifyArtifact } from "../../machines/owned-process.ts";
import { boardIdentity } from "../../clients/board-requests.ts";
import { Receipt } from "../../receipt.ts";
import { RunLifetime } from "../../run-lifetime.ts";
import {
  driveLiveHeld,
  driveQueueHeld,
  requireNoD1Turns,
  observedD1Turns,
} from "./d1-mail-stages.ts";
import { createD1Project } from "./d1-projects.ts";
import { driveD1OnlineTiming, reportD1OnlineTiming } from "./d1-online-timing.ts";
import { requireD1RuntimeDirectory } from "./d1-runtime-allocation.ts";
import { driveMaterializedD1Mail } from "./d1-materialized-mail.ts";
import type { D1Machine, D1HarnessDeclaration, D1HarnessJourney } from "./d1-harness-plan.ts";
import { startD1Machine } from "./d1-machine.ts";
import {
  d1FixtureEnvironment,
  writeD1FixturePlan,
  stopD1ProtocolProcess,
  readD1Reply,
} from "./d1-process.ts";

async function driveSelected(
  declaration: D1HarnessDeclaration,
  machines: readonly D1Machine[],
  receipt: Receipt,
): Promise<readonly string[]> {
  const sender = machines[0],
    recipient = machines[1];
  if (sender === undefined || recipient === undefined)
    throw new Error("D1 message participants absent");
  if (declaration.materialization !== undefined)
    return driveMaterializedD1Mail({ declaration, machines, receipt });
  if (declaration.twoProjects) {
    await createD1Project(sender, "P1", receipt);
    await createD1Project(recipient, "P2", receipt);
  }
  if (declaration.queueScenario === "offline-reopen")
    await driveQueueHeld({ sender, recipient, receipt });
  if (declaration.queueScenario === "online-held")
    await driveLiveHeld({ sender, recipient, receipt });
  if (declaration.queueScenario === "online-timing")
    return driveD1OnlineTiming({ sender, recipient, receipt });
  await requireNoD1Turns(machines, receipt);
  return [];
}

async function prepareD1Evidence(declaration: D1HarnessDeclaration): Promise<Receipt> {
  if (
    (await realpath(declaration.repository)) !== process.cwd() ||
    Date.parse(declaration.deadline) <= Date.now()
  )
    throw new Error("current canonical worktree and live deadline required");
  if (!declaration.evidenceDirectory.startsWith(`${declaration.repository}/tmp/d1-verify/`))
    throw new Error("D1 evidence must stay inside its granted task root");
  if (declaration.runtimeDirectory !== undefined) {
    await requireD1RuntimeDirectory(declaration.runtimeDirectory);
    if ((await readdir(declaration.runtimeDirectory)).length !== 0)
      throw new Error("D1 short runtime directory must be fresh and empty");
  }
  await mkdir(`${declaration.repository}/tmp/d1-verify/allocation`, {
    recursive: true,
    mode: 0o700,
  });
  await mkdir(declaration.evidenceDirectory, { mode: 0o700 });
  const receipt = new Receipt(declaration.evidenceDirectory);
  await receipt.save("declaration.json", declaration);
  return receipt;
}

async function validateD1Journey(
  declaration: D1HarnessDeclaration,
  journey?: D1HarnessJourney,
): Promise<void> {
  if (
    journey !== undefined &&
    (declaration.materialization !== undefined ||
      declaration.twoProjects ||
      declaration.queueScenario !== "none")
  )
    throw new Error(
      "a custom no-model journey cannot combine legacy drive selectors or materialization",
    );
  if (journey?.inboxAcceptedBarrierSocket !== undefined) {
    const path = journey.inboxAcceptedBarrierSocket;
    await privateDirectory(dirname(path));
    const endpoint = await lstat(path);
    if (
      !path.startsWith("/") ||
      Buffer.byteLength(path) > 99 ||
      !endpoint.isSocket() ||
      endpoint.uid !== process.getuid?.()
    )
      throw new Error("owned short receiver barrier socket required");
  }
}

async function reportD1AfterStop(input: {
  declaration: D1HarnessDeclaration;
  journey: D1HarnessJourney | undefined;
  machines: readonly D1Machine[];
  receipt: Receipt;
  timingItems: readonly string[];
  cleanupSucceeded: boolean;
  failure: string | null;
}): Promise<string | null> {
  const { declaration, journey, machines, receipt, timingItems } = input;
  let failure = input.failure;
  if (journey !== undefined && input.cleanupSucceeded) {
    try {
      await journey.ownersStopped({ machines, receipt });
    } catch (error: unknown) {
      const observationFailure = String(error);
      await receipt.record("d1JourneyPostStopFailure", { failure: observationFailure });
      failure ??= observationFailure;
    }
  }
  if (timingItems.length !== 0 && failure === null && input.cleanupSucceeded)
    failure = await reportD1OnlineTiming({
      repository: declaration.repository,
      recipientDirectory: machines[1]?.serviceDirectory,
      runtimeDirectory: declaration.runtimeDirectory,
      items: timingItems,
      receipt,
    });
  return failure;
}

export async function runD1Harness(
  declaration: D1HarnessDeclaration,
  journey?: D1HarnessJourney,
): Promise<void> {
  await validateD1Journey(declaration, journey);
  const receipt = await prepareD1Evidence(declaration);
  if (journey !== undefined) await receipt.save("journey.json", journey.declaration);
  const lifetime = new RunLifetime(receipt);
  const machines: D1Machine[] = [];
  let failure: string | null = null;
  let timingItems: readonly string[] = [];
  try {
    await qualifyArtifact(declaration.driver);
    await qualifyArtifact(declaration.cli);
    const centralRoot = `${declaration.repository}/tmp/d1-verify/allocation/central-${boardIdentity()}`;
    const centralPlan = `${receipt.directory}/central.json`;
    await writeD1FixturePlan(centralPlan, {
      repository: declaration.repository,
      root: centralRoot,
      deadline: declaration.deadline,
    });
    const central = new OwnedProcess({
      executable: declaration.driver.path,
      arguments: ["d1-central", centralPlan],
      environment: d1FixtureEnvironment(`${centralRoot}/unused-codex`, centralRoot),
      cwd: declaration.repository,
      name: "central",
      receipt,
    });
    lifetime.own("central", { close: (): Promise<void> => stopD1ProtocolProcess(central) });
    journey?.centralStarted(central);
    const ready = z
      .object({ pid: z.bigint(), origin: z.url() })
      .parse(await readD1Reply(central, "d1CentralReady"));
    if (ready.pid !== BigInt(central.pid))
      throw new Error("D1 central readiness names another process");
    for (const name of ["a", "b", "c"] as const)
      machines.push(
        await startD1Machine({
          declaration,
          name,
          central,
          origin: ready.origin,
          centralRoot,
          receipt,
          lifetime,
          journey,
        }),
      );
    await receipt.require(
      "three independently enrolled installations",
      new Set(machines.map((machine): string => machine.recipient.endpoint.serviceId)).size === 3,
    );
    const output = `${centralRoot}/sockets.json`;
    central.child.stdin?.write(`${encode({ kind: "sockets", output })}\n`);
    await readD1Reply(central, "d1Sockets");
    await receipt.save("central-sockets.json", decode(await readFile(output, "utf8")));
    await receipt.record("d1HarnessReady", {
      installations: machines.map((machine) => machine.recipient),
      modelInputSent: false,
      productLayoutClaim: "not evaluated on old composition",
    });
    if (journey === undefined) timingItems = await driveSelected(declaration, machines, receipt);
    else await journey.drive({ machines, receipt, central, origin: ready.origin, centralRoot });
  } catch (error: unknown) {
    failure = error instanceof Error ? error.message : "unknown harness failure";
    await receipt.save("failure.json", { failure });
  }
  const cleanupFailures = await lifetime.close();
  failure = await reportD1AfterStop({
    declaration,
    journey,
    machines,
    receipt,
    timingItems,
    cleanupSucceeded: cleanupFailures.length === 0,
    failure,
  });
  const actualTurnsStarted = machines.reduce(
    (sum, machine): number => sum + observedD1Turns(machine),
    0,
  );
  const permittedTurns =
    declaration.materialization === undefined ? 0 : declaration.materialization.reloadHeld ? 2 : 1;
  if (actualTurnsStarted !== permittedTurns && failure === null)
    failure = "unexpected native turn started from a side effect";
  await receipt.save("result.json", {
    kind: "d1Harness",
    machines: machines.length,
    failure,
    cleanupFailures,
    modelInputSent: actualTurnsStarted !== 0,
    actualTurnsStarted,
    productObservations: "open",
    stagingExecuted: false,
  });
  if (failure !== null || cleanupFailures.length !== 0)
    throw new Error("D1 harness qualification failed; retained evidence");
}
