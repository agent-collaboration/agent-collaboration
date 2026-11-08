/** Stdio protocol and exact owned shutdown of D1 fixture processes. */
import { writeFile } from "node:fs/promises";
import { z } from "zod";
import { encode, decode } from "../../exact-json.ts";
import { OwnedProcess } from "../../machines/owned-process.ts";
import { lookupProcessIdentity, signalEachOwned } from "../runtime/process-ownership.ts";

export function d1FixtureEnvironment(codexHome: string, temporary: string): NodeJS.ProcessEnv {
  return {
    HOME: process.env.HOME,
    PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
    CODEX_HOME: codexHome,
    TMPDIR: temporary,
    RUST_LOG: "off",
    CODEX_ROUTER_DEBUG_READINESS_TIMING: "1",
  };
}

export async function writeD1FixturePlan(path: string, value: unknown): Promise<void> {
  await writeFile(path, `${encode(value)}\n`, { mode: 0o600, flag: "wx" });
}

export async function stopD1ProtocolProcess(process: OwnedProcess): Promise<void> {
  const known = new Map(
    process.capture().flatMap((pid) => {
      const identity = lookupProcessIdentity(pid);
      return identity === null ? [] : [[pid, identity] as const];
    }),
  );
  await process.receipt.record("d1CleanupInventory", { identities: [...known.values()] });
  process.child.stdin?.end();
  try {
    await process.stop();
  } catch (failure: unknown) {
    const sent = signalEachOwned(known, lookupProcessIdentity, (pid): void => {
      globalThis.process.kill(pid, "SIGTERM");
    });
    await process.receipt.record("d1CleanupFallback", { signal: "SIGTERM", ...sent });
    try {
      await process.stop();
    } catch {
      const killed = signalEachOwned(known, lookupProcessIdentity, (pid): void => {
        globalThis.process.kill(pid, "SIGKILL");
      });
      await process.receipt.record("d1CleanupFallback", { signal: "SIGKILL", ...killed });
      await process.stop();
    }
    throw failure;
  }
  if (process.child.exitCode !== 0) throw new Error("D1 owned process did not exit cleanly");
}

export async function readD1Reply(process: OwnedProcess, kind: string): Promise<unknown> {
  const result = decode(await process.line(90));
  z.object({ kind: z.literal(kind) }).parse(result);
  return result;
}
