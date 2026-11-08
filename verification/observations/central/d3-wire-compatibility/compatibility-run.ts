/** Run with verification/run.ts --d3-wire PLAN. Drives only declared isolated debug instances. */
import { mkdir, mkdtemp, readFile, realpath } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { openSync, closeSync } from "node:fs";
import { z } from "zod";
import type { CompatibilityPlan } from "./compatibility-plan.ts";
import { FixtureInstallation, freePort } from "./fixture-installation.ts";
import { driveCompatibilityJourneys, observeCompatibilityPair } from "./compatibility-journeys.ts";
import { boardIdentity } from "../../../clients/board-requests.ts";
import { decode } from "../../../exact-json.ts";
import {
  command,
  OwnedProcess,
  privateDirectory,
  qualifyArtifact,
  fingerprint,
} from "../../../machines/owned-process.ts";
import { Receipt } from "../../../receipt.ts";

async function prepareCompatibility(plan: CompatibilityPlan): Promise<{
  receipt: Receipt;
  runtime: string;
  centralRoot: string;
  centralPlan: string;
}> {
  const receipt = new Receipt(plan.evidenceDirectory);
  await mkdir(receipt.directory, { mode: 0o700 });
  await receipt.save("declaration.json", plan);
  const source = await command("/usr/bin/git", ["rev-parse", "HEAD"], { cwd: plan.repository });
  if (source.exit !== 0 || source.stdout.trim() !== plan.sourceHead)
    throw new Error("central source pin changed");
  if ((await fingerprint(plan.oldSourceArchive.path)) !== plan.oldSourceArchive.sha256)
    throw new Error("old-source archive hash mismatch");
  const archiveDescriptor = openSync(plan.oldSourceArchive.path, "r");
  let archivedCommit: string;
  try {
    // get-tar-commit-id intentionally exits after the initial tar header. A
    // pipe containing the whole archive can fail EPIPE after that valid exit.
    archivedCommit = execFileSync("/usr/bin/git", ["get-tar-commit-id"], {
      stdio: [archiveDescriptor, "pipe", "pipe"],
      encoding: "utf8",
    }).trim();
  } finally {
    closeSync(archiveDescriptor);
  }
  if (archivedCommit !== plan.oldSourceHead)
    throw new Error("old source is not the adopted release tag");
  for (const artifact of [plan.oldRouter, plan.oldCli, plan.centralDriver, plan.native])
    await qualifyArtifact(artifact);
  const runtime = await realpath(await mkdtemp("/private/tmp/d3v-"));
  await privateDirectory(runtime);
  await receipt.save("runtime-allocation.json", {
    runtime,
    normalHomes: "not used",
    keychain: "forbidden",
  });
  const allocation = `${plan.repository}/tmp/d1-verify/allocation`;
  await mkdir(allocation, { recursive: true, mode: 0o700 });
  const centralRoot = `${allocation}/d3-${boardIdentity()}`;
  const centralPlan = `${receipt.directory}/central-plan.json`;
  await receipt.save("central-plan.json", {
    repository: plan.repository,
    root: centralRoot,
    deadline: plan.deadline,
  });
  return { receipt, runtime, centralRoot, centralPlan };
}

export async function runCompatibility(plan: CompatibilityPlan): Promise<void> {
  const { receipt, runtime, centralRoot, centralPlan } = await prepareCompatibility(plan);
  let central: OwnedProcess | undefined;
  const machines: FixtureInstallation[] = [];
  let failure: unknown;
  const cleanupFailures: string[] = [];
  try {
    await qualifyArtifact(plan.centralDriver);
    central = new OwnedProcess({
      executable: plan.centralDriver.path,
      arguments: ["d1-central", centralPlan],
      environment: {
        HOME: process.env.HOME,
        PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
        TMPDIR: runtime,
        RUST_LOG: "off",
      },
      cwd: plan.repository,
      name: "d3-central",
      receipt,
    });
    const ready = z
      .object({ kind: z.literal("d1CentralReady"), origin: z.url(), pid: z.bigint() })
      .parse(decode(await central.line(60)));
    if (ready.pid !== BigInt(central.pid) || new URL(ready.origin).hostname !== "127.0.0.1")
      throw new Error("central identity/readiness mismatch");
    central.capture();
    const owner = z
      .object({ origin: z.literal(ready.origin), assertion: z.string() })
      .parse(decode(await readFile(`${centralRoot}/owner-fixture.json`, "utf8")));
    for (const name of ["a", "b"]) {
      const machine = await FixtureInstallation.prepare({
        name,
        runtime,
        port: await freePort(),
        plan,
        receipt,
        central: ready.origin,
        ownerAssertion: owner.assertion,
      });
      machines.push(machine);
      await machine.start();
    }
    const [a, b] = machines;
    if (
      a === undefined ||
      b === undefined ||
      a.recipient?.endpoint.serviceId === b.recipient?.endpoint.serviceId
    )
      throw new Error("two distinct old-source installations required");
    await driveCompatibilityJourneys({ a, b, receipt });
    await observeCompatibilityPair({ a, b, receipt }, "final-completion");
    for (const artifact of [plan.oldRouter, plan.oldCli, plan.centralDriver])
      await qualifyArtifact(artifact);
    const final = await command("/usr/bin/git", ["rev-parse", "HEAD"], { cwd: plan.repository });
    if (final.stdout.trim() !== plan.sourceHead)
      throw new Error("target changed during observation");
  } catch (error: unknown) {
    failure = error;
    await receipt.save("failure.json", {
      message: String(error),
      stack: error instanceof Error ? error.stack : undefined,
    });
  } finally {
    cleanupFailures.push(...(await closeCompatibility({ machines, central, receipt })));
    if (failure === undefined) failure = await finalCompatibilityFailure({ machines, receipt });
    await receipt.save("result.json", {
      passed: failure === undefined && cleanupFailures.length === 0,
      buildLabel: "released 0.1.125 source, debug build",
      executed: true,
      cleanupFailures,
      retainedRuntime: runtime,
      retainedCentral: centralRoot,
      notCovered: [
        "release artifact runtime",
        "staging",
        "model input",
        "D4/D5 flight/window/hour/wakes",
      ],
    });
  }
  if (failure !== undefined || cleanupFailures.length > 0)
    throw new Error("D3 compatibility incomplete; inspect retained result/failure");
}

async function finalCompatibilityFailure(input: {
  machines: readonly FixtureInstallation[];
  receipt: Receipt;
}): Promise<unknown> {
  try {
    const a = input.machines.find((machine): boolean => machine.input.name === "a");
    const b = input.machines.find((machine): boolean => machine.input.name === "b");
    if (a === undefined || b === undefined) throw new Error("final pair evidence incomplete");
    await observeCompatibilityPair({ a, b, receipt: input.receipt }, "final-after-cleanup");
    return undefined;
  } catch (error: unknown) {
    await input.receipt.save("final-audit-failure.json", { message: String(error) });
    return error;
  }
}

async function closeCompatibility(input: {
  machines: readonly FixtureInstallation[];
  central: OwnedProcess | undefined;
  receipt: Receipt;
}): Promise<readonly string[]> {
  const failures: string[] = [];
  for (const machine of [...input.machines].reverse()) {
    try {
      await machine.close();
    } catch (error: unknown) {
      failures.push(String(error));
    }
    await input.receipt.save(`wire-${machine.input.name}.json`, {
      snapshot: machine.wire.snapshot(),
      frames: machine.wire.observations(),
      audit: machine.gate?.audit.snapshot(),
    });
  }
  if (input.central !== undefined) {
    try {
      input.central.child.stdin?.end();
      await input.central.stop();
    } catch (error: unknown) {
      failures.push(String(error));
    }
  }
  return failures;
}
