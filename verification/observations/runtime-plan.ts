import { chmodSync, closeSync, openSync, readFileSync, writeFileSync } from "node:fs";
import { spawn, type ChildProcess } from "node:child_process";
import { once } from "node:events";
import { constants } from "node:os";
import { performance } from "node:perf_hooks";
import { z } from "zod";
import { decode } from "../exact-json.ts";
import { prettyEvidenceJson } from "../evidence-json.ts";
import { ConfigurationChanged } from "./readonly-codex-home.ts";
import { ProcessIdentityError } from "./process-ownership.ts";

export const runtimePlanSchema = z.strictObject({
  repository: z.string(),
  source_head: z.string(),
  qualification: z.string(),
  router_root: z.string(),
  evidence: z.string(),
  port: z.coerce.number().int().min(1).max(65535),
  expected_version: z.string(),
  profile: z.string(),
  codex_config: z.string(),
  native_cwd: z.string(),
  claude_node: z.string(),
  claude_adapter: z.string(),
  claude_executable: z.string(),
  cursor_executable: z.string(),
  deadline: z.string(),
});
export type RuntimePlan = z.infer<typeof runtimePlanSchema>;

export class AllocationTimeout extends Error {
  override name = "TimeoutError";
}
export class ProcessWaitTimeout extends Error {
  override name = "TimeoutExpired";
}

export function deadlineEpoch(plan: RuntimePlan): number {
  return /(?:Z|[+-]\d{2}:?\d{2})$/.test(plan.deadline) ? Date.parse(plan.deadline) : Number.NaN;
}

export function loadRuntimePlan(path: string): RuntimePlan {
  return runtimePlanSchema.parse(decode(readFileSync(path, "utf8")));
}
export function saveRecord(path: string, value: unknown): void {
  writeFileSync(path, `${prettyEvidenceJson(value)}\n`, { flag: "wx", mode: 0o600 });
  chmodSync(path, 0o600);
}

export function errorCategory(error: unknown): string {
  if (error instanceof ProcessIdentityError) return error.category;
  if (error instanceof ConfigurationChanged) return "ConfigurationChanged";
  if (error instanceof ProcessWaitTimeout || error instanceof AllocationTimeout) return error.name;
  if (error instanceof z.ZodError) return "ValidationError";
  const code =
    error !== null && typeof error === "object" && "code" in error ? error.code : undefined;
  if (code === "ETIMEDOUT") return "TimeoutExpired";
  if (code === "ENOENT") return "FileNotFoundError";
  if (code === "EPERM" || code === "EACCES") return "PermissionError";
  if (error instanceof Error && error.name !== "Error") return error.name;
  return "RuntimeError";
}

export function processExit(child: ChildProcess): number | null {
  if (child.exitCode !== null) return child.exitCode;
  if (child.signalCode === null) return null;
  return -constants.signals[child.signalCode];
}

export async function waitForProcess(child: ChildProcess, timeoutMs: number): Promise<number> {
  const exited = processExit(child);
  if (exited !== null) return exited;
  return new Promise((resolve, reject): void => {
    const onExit = (): void => {
      clearTimeout(timer);
      child.off("error", onError);
      const code = processExit(child);
      if (code === null) reject(new Error("child exit status unavailable"));
      else resolve(code);
    };
    const onError = (error: Error): void => {
      clearTimeout(timer);
      child.off("exit", onExit);
      reject(error);
    };
    const timer = setTimeout((): void => {
      child.off("exit", onExit);
      child.off("error", onError);
      reject(new ProcessWaitTimeout("owned child exit deadline"));
    }, timeoutMs);
    child.once("exit", onExit);
    child.once("error", onError);
  });
}

export interface RuntimeLaunch {
  readonly command: readonly string[];
  readonly environment?: NodeJS.ProcessEnv;
  readonly stdout: string;
  readonly stderr: string;
}

export async function launchRuntimeChild(input: RuntimeLaunch): Promise<ChildProcess> {
  const [binary, ...arguments_] = input.command;
  if (binary === undefined) throw new Error("runtime executable missing");
  const stdout = openSync(input.stdout, "wx");
  try {
    const stderr = openSync(input.stderr, "wx");
    try {
      const child = spawn(binary, arguments_, {
        env: input.environment,
        stdio: ["inherit", stdout, stderr],
        detached: true,
      });
      await once(child, "spawn");
      return child;
    } finally {
      closeSync(stderr);
    }
  } finally {
    closeSync(stdout);
  }
}

export const runtimeClock = {
  wallNow: (): number => Date.now(),
  monotonicNow: (): number => performance.now(),
  delay: (milliseconds: number): Promise<void> =>
    new Promise((resolve): void => {
      setTimeout(resolve, milliseconds);
    }),
};
