/** Bootstrap two local enrolled Hosts and Tokio central. No automation or model is driven.
 * Usage: node verification/observations/central-automations-rig.ts PLAN.json
 * The plan binds the signed example, source head, private runtime and evidence directories.
 * Readiness and public installation readback qualify setup only; all V rows remain unknown.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { createWriteStream } from "node:fs";
import { mkdir, readFile, realpath, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { createInterface } from "node:readline";
import { finished } from "node:stream/promises";
import { fileURLToPath } from "node:url";
import { z } from "zod";
import {
  centralAutomationsPlanSchema,
  type CentralAutomationsPlan,
} from "./central-automations-plan.ts";
import { command, privateDirectory, qualifyArtifact } from "../machines/owned-process.ts";
import { RouterMcp } from "../clients/router-mcp.ts";
import { decode, encode } from "../exact-json.ts";

interface OwnedRigChild {
  readonly child: ChildProcess;
  readonly ready: Promise<unknown>;
  readonly done: Promise<{ exit: number | null; signal: string | null }>;
  readonly capture: Promise<void>;
  readonly mode: string;
}

function startChild(input: {
  plan: CentralAutomationsPlan;
  runtimePlan: string;
  mode: string;
  readyKind: string;
}): OwnedRigChild {
  const output = createWriteStream(`${input.plan.evidence}/${input.mode}.stdout`, {
    flags: "wx",
    mode: 0o600,
  });
  const errors = createWriteStream(`${input.plan.evidence}/${input.mode}.stderr`, {
    flags: "wx",
    mode: 0o600,
  });
  const environment = { ...process.env };
  delete environment.CODEX_ROUTER_USE_HOME_DEFAULT;
  delete environment.CODEX_ROUTER_HOST_INHERITED_LOCK;
  const child = spawn(input.plan.executable.path, [input.mode, input.runtimePlan], {
    cwd: input.plan.repository,
    env: environment,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const done = new Promise<{ exit: number | null; signal: string | null }>((resolve): void => {
    child.once("close", (exit, signal): void => resolve({ exit, signal }));
  });
  child.stdout?.pipe(output);
  child.stderr?.pipe(errors);
  const capture = Promise.all([finished(output), finished(errors)]).then((): void => {});
  const ready = new Promise<unknown>((resolve, reject): void => {
    child.once("error", reject);
    child.once("close", (): void => reject(new Error(`${input.mode} exited before readiness`)));
    if (child.stdout === null) {
      reject(new Error("owned stdout unavailable"));
      return;
    }
    const lines = createInterface({ input: child.stdout });
    lines.on("line", (line): void => {
      try {
        const observation = z
          .object({ kind: z.string(), pid: z.number().int().positive() })
          .parse(JSON.parse(line));
        if (observation.kind === input.readyKind) {
          if (observation.pid !== child.pid)
            throw new Error("readiness belongs to a different PID");
          resolve(JSON.parse(line));
        }
      } catch (error: unknown) {
        reject(error);
      }
    });
  });
  return { child, ready, done, capture, mode: input.mode };
}

async function bounded<TValue>(operation: Promise<TValue>, signal: AbortSignal): Promise<TValue> {
  signal.throwIfAborted();
  let abortListener: (() => void) | undefined;
  try {
    return await Promise.race([
      operation,
      new Promise<never>((_, reject): void => {
        abortListener = (): void => reject(signal.reason);
        signal.addEventListener("abort", abortListener, { once: true });
      }),
    ]);
  } finally {
    if (abortListener !== undefined) signal.removeEventListener("abort", abortListener);
  }
}

async function stopChild(owned: OwnedRigChild): Promise<unknown> {
  if (owned.child.exitCode === null && owned.child.signalCode === null) {
    if (owned.mode === "central") owned.child.stdin?.end(`${encode({ kind: "stop" })}\n`);
    else owned.child.kill("SIGTERM");
  }
  const result = await bounded(owned.done, AbortSignal.timeout(20_000));
  await owned.capture;
  if (result.exit !== 0 || result.signal !== null)
    throw new Error(`${owned.mode} failed to drain: ${encode(result)}`);
  return { mode: owned.mode, pid: owned.child.pid, ...result };
}

export async function bootstrapCentralAutomations(plan: CentralAutomationsPlan): Promise<void> {
  if ((await realpath(plan.repository)) !== plan.repository)
    throw new Error("canonical checkout required");
  const head = await command("/usr/bin/git", ["rev-parse", "HEAD"], { cwd: plan.repository });
  if (head.exit !== 0 || head.stdout.trim() !== plan.sourceHead)
    throw new Error("declared source head changed");
  await qualifyArtifact(plan.executable);
  // Only canonical existing parents may receive new owned directories.
  for (const destination of [plan.directory, plan.evidence]) {
    if ((await realpath(dirname(destination))) !== dirname(destination))
      throw new Error("canonical existing parent required");
  }
  await mkdir(plan.evidence, { mode: 0o700 });
  await privateDirectory(plan.evidence);
  await writeFile(`${plan.evidence}/declaration.json`, `${encode(plan)}\n`, {
    mode: 0o600,
    flag: "wx",
  });
  const children: OwnedRigChild[] = [];
  const ready: unknown[] = [];
  const identities: unknown[] = [];
  const stopped: unknown[] = [];
  let failure: unknown;
  const cleanupFailures: unknown[] = [];
  try {
    await mkdir(plan.directory, { mode: 0o700 });
    await privateDirectory(plan.directory);
    const runtimePlan = `${plan.evidence}/runtime-plan.json`;
    await writeFile(
      runtimePlan,
      `${encode({ repository: plan.repository, directory: plan.directory, claude_acp: null })}\n`,
      { mode: 0o600, flag: "wx" },
    );
    for (const mode of ["central", "host-a", "host-b"]) {
      const owned = startChild({
        plan,
        runtimePlan,
        mode,
        readyKind:
          mode === "central" ? "centralAutomationsCentralReady" : "centralAutomationsHostReady",
      });
      children.push(owned);
      ready.push(await bounded(owned.ready, AbortSignal.timeout(45_000)));
    }
    for (const machine of ["a", "b"]) {
      const manifest = z
        .object({ mcp: z.object({ url: z.string() }) })
        .parse(decode(await readFile(`${plan.directory}/${machine}/service.json`, "utf8")));
      const identity = z
        .object({ serviceId: z.string(), peerId: z.string() })
        .parse(await new RouterMcp(manifest.mcp.url).call("installation_show", {}));
      identities.push(identity);
    }
    if (
      new Set(
        identities.map(
          (identity): string => z.object({ serviceId: z.string() }).parse(identity).serviceId,
        ),
      ).size !== 2
    )
      throw new Error("Hosts share one installation identity");
    await writeFile(
      `${plan.evidence}/ready.json`,
      `${encode({ ready, identities, productRowsObserved: [], modelInputsSent: 0 })}\n`,
      { mode: 0o600, flag: "wx" },
    );
  } catch (error: unknown) {
    failure = error;
  } finally {
    for (const child of [...children].reverse()) {
      try {
        stopped.push(await stopChild(child));
      } catch (error: unknown) {
        cleanupFailures.push(String(error));
      }
    }
    await writeFile(
      `${plan.evidence}/settlement.json`,
      `${encode({ stopped, failure: failure === undefined ? null : String(failure), cleanupFailures, productRowsObserved: [] })}\n`,
      { mode: 0o600, flag: "wx" },
    );
  }
  if (failure !== undefined || cleanupFailures.length > 0)
    throw new AggregateError(
      [...(failure === undefined ? [] : [failure]), ...cleanupFailures],
      "apparatus bootstrap failed; no product observation",
    );
}

async function main(): Promise<void> {
  const arguments_ = process.argv.slice(2);
  if (arguments_.length === 1 && arguments_[0] === "--help") {
    process.stdout.write(
      "Usage: node verification/observations/central-automations-rig.ts PLAN.json\nBootstrap only: two isolated enrolled Hosts plus real local Tokio central; no models or V-row credit. Plan schema: central-automations-plan.ts. Retains readiness, public identities, stdout/stderr and exact owned shutdown.\n",
    );
    return;
  }
  const [path] = arguments_;
  if (arguments_.length !== 1 || path === undefined)
    throw new Error("one plan required; see --help");
  await bootstrapCentralAutomations(
    centralAutomationsPlanSchema.parse(JSON.parse(await readFile(path, "utf8"))),
  );
}
if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error: unknown): void => {
    process.stderr.write(`${String(error)}\n`);
    process.exitCode = 1;
  });
}
