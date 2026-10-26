import { chmodSync, existsSync, mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { spawn, type ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";
import { once } from "node:events";
import { decode } from "../../exact-json.ts";
import { binaryQualificationSchema, type IdentityRecord } from "../evidence-models.ts";
import { lookupProcessIdentity, sameIdentity } from "../process-ownership.ts";
import { processOperations, processTable, checkRootConsumers } from "../runtime-processes.ts";
import { runtimeOperations, runLifecycle, type RuntimeOperations } from "../owned-runtime.ts";
import {
  ProcessWaitTimeout,
  processExit,
  runtimePlanSchema,
  saveRecord,
  waitForProcess,
  type RuntimeLaunch,
  type RuntimePlan,
} from "../runtime-plan.ts";
import { sha256 } from "../runtime-qualification.ts";

export async function harmlessChild(
  graceful: boolean = false,
): Promise<{ readonly child: ChildProcess; readonly identity: IdentityRecord }> {
  lookupProcessIdentity(process.pid);
  const child = spawn(
    process.execPath,
    [
      new URL("../../fixtures/harmless-owned-child.ts", import.meta.url).pathname,
      ...(graceful ? ["--graceful"] : []),
    ],
    { stdio: ["pipe", "pipe", "pipe"], detached: true },
  );
  await once(child, "spawn");
  const lines = createInterface({ input: child.stdout });
  try {
    const [line] = await once(lines, "line");
    if (line !== "READY") throw new Error("wrong harmless fixture readiness");
  } finally {
    lines.close();
  }
  if (child.pid === undefined) throw new Error("fixture child PID unavailable");
  const identity = lookupProcessIdentity(child.pid);
  if (identity === null) throw new Error("fixture child identity unavailable");
  return { child, identity };
}

export async function releaseHarmlessChild(
  child: ChildProcess,
  recorded: IdentityRecord,
): Promise<void> {
  if (processExit(child) === null) {
    if (child.pid === undefined) throw new Error("fixture child PID unavailable");
    if (sameIdentity(lookupProcessIdentity(child.pid), recorded)) child.kill("SIGTERM");
    else await waitForProcess(child, 5000);
  }
  await waitForProcess(child, 5000);
  child.stdin?.destroy();
  child.stdout?.destroy();
  child.stderr?.destroy();
}

export class LifecycleFixture {
  readonly plan: RuntimePlan;
  readonly ownerProfile = 'url="http://127.0.0.1:43127/v1"\n# owner edit\n';
  readonly ownerConfig = 'model = "owner-selected-model"\n';
  readonly phases: string[] = [];
  readonly launched: string[] = [];
  readonly children: { child: ChildProcess; identity: IdentityRecord }[] = [];
  private readonly globalBin: string;
  private readonly nowValue = Date.now();
  private ready = false;
  private prelaunchExpired = false;
  private inventoryTimeoutEmitted = false;
  private scenario: string;

  constructor(root: string, scenario: string, mismatch: boolean = false) {
    this.scenario = scenario;
    const routerRoot = join(root, "router-root");
    mkdirSync(routerRoot, { mode: 0o700 });
    const qualified = join(root, "qualified");
    mkdirSync(qualified);
    const binaries = ["codex-router", "agent-collaboration", "agent-sessions"].map((target) => {
      const path = join(qualified, target);
      writeFileSync(path, "apparatus fixture; never executed");
      return { target, path, sha256: sha256(path), testGuardFeatures: [] };
    });
    const qualification = join(root, "qualification.json");
    writeFileSync(qualification, JSON.stringify({ binaries }));
    const profile = join(root, "debug-profile");
    writeFileSync(profile, 'url="http://127.0.0.1:43127/v1"\n');
    const config = join(root, "config.toml");
    writeFileSync(
      config,
      `model = "fixture-model"\n[projects.${JSON.stringify(root)}]\ntrust_level = "trusted"\n`,
    );
    this.globalBin = join(root, "global-bin");
    mkdirSync(this.globalBin);
    const claude = join(this.globalBin, "claude");
    writeFileSync(claude, "owned executable fixture; never executed");
    chmodSync(claude, 0o700);
    const planned = mismatch ? join(root, "different-claude") : claude;
    if (mismatch) {
      writeFileSync(planned, "different owned fixture");
      chmodSync(planned, 0o700);
    }
    this.plan = runtimePlanSchema.parse({
      repository: root,
      source_head: "0".repeat(40),
      qualification,
      router_root: routerRoot,
      evidence: join(root, "evidence"),
      port: 43128,
      expected_version: "fixture",
      profile,
      codex_config: config,
      native_cwd: root,
      claude_node: process.execPath,
      claude_adapter: join(root, "unused-adapter"),
      claude_executable: planned,
      cursor_executable: process.execPath,
      deadline: new Date(this.nowValue + 3_600_000).toISOString(),
    });
  }

  private wallNow = (): number => {
    if (this.prelaunchExpired || (this.ready && this.scenario === "deadline"))
      return Date.parse(this.plan.deadline) + 1000;
    return this.nowValue;
  };

  private table = (): ReturnType<typeof processTable> => {
    if (this.scenario === "inventory-timeout" && this.ready && !this.inventoryTimeoutEmitted) {
      this.inventoryTimeoutEmitted = true;
      throw new ProcessWaitTimeout("controlled inventory timeout");
    }
    return processTable();
  };

  private launch = async (input: RuntimeLaunch): Promise<ChildProcess> => {
    const kind = input.command[1];
    if (kind === undefined) throw new Error("fixture command kind missing");
    this.launched.push(kind);
    if (kind === "host") {
      this.phases.push(this.phases.length === 0 ? "A" : "B");
      const publication = join(this.plan.router_root, "agent-communication");
      mkdirSync(publication, { recursive: true });
      writeFileSync(
        join(publication, "service.json"),
        JSON.stringify({ serviceVersion: "fixture" }),
      );
    }
    const owned = await harmlessChild(true);
    this.children.push(owned);
    return owned.child;
  };

  private save = (path: string, value: unknown): void => {
    saveRecord(path, value);
    const name = basename(path);
    if (name === "router-ready.json" && this.scenario === "profile-before-host")
      writeFileSync(this.plan.profile, this.ownerProfile);
    if (name === "router-ready.json" && this.scenario === "config-before-host")
      writeFileSync(this.plan.codex_config, this.ownerConfig);
    if (name === "host-A-ready.json") this.hostReady();
    if (name === "host-B-ready.json") writeFileSync(join(this.plan.evidence, "stop.json"), "{}");
    if (name === "host-A-settlement.json" || name === "host-B-settlement.json") {
      const publication = join(this.plan.router_root, "agent-communication/service.json");
      if (existsSync(publication)) unlinkSync(publication);
      if (this.scenario === "reopen-expired") this.scenario = "deadline";
    }
  };

  private hostReady(): void {
    this.ready = true;
    if (this.scenario === "profile-after-host") writeFileSync(this.plan.profile, this.ownerProfile);
    if (this.scenario === "config-after-host")
      writeFileSync(this.plan.codex_config, this.ownerConfig);
    if (this.scenario === "external-desktop-after-host")
      writeFileSync(
        this.plan.codex_config,
        `${readFileSync(this.plan.codex_config, "utf8")}[desktop.preferences]\nopen = true\n`,
      );
    if (this.scenario === "early-exit") this.children[1]?.child.stdin?.end();
    if (
      [
        "stop",
        "reopen",
        "reopen-expired",
        "profile-before-host",
        "profile-after-host",
        "config-before-host",
        "config-after-host",
        "external-desktop-after-host",
      ].includes(this.scenario)
    )
      writeFileSync(
        join(
          this.plan.evidence,
          this.scenario === "reopen" || this.scenario === "reopen-expired"
            ? "reopen.json"
            : "stop.json",
        ),
        "{}",
      );
  }

  private consumers = (root: string, allowedRouter: number | null = null): void => {
    checkRootConsumers(root, allowedRouter);
    if (
      this.scenario === "inspection-expired" &&
      allowedRouter !== null &&
      this.phases.length === 0
    )
      this.prelaunchExpired = true;
  };

  async drive(): Promise<Error | null> {
    const overrides = {
      wallNow: this.wallNow,
      delay: (): Promise<void> =>
        new Promise((resolve): void => {
          setImmediate(resolve);
        }),
      process: { ...processOperations, table: this.table },
      gitHead: (): string => this.plan.source_head,
      assertPortAvailable: async (): Promise<void> => {},
      launch: this.launch,
      save: this.save,
      health: async (): Promise<Record<string, unknown>> => ({ binary_version: "fixture" }),
      qualifyBinaries: (path: string): ReturnType<typeof runtimeOperations.qualifyBinaries> => {
        const records = binaryQualificationSchema.parse(decode(readFileSync(path, "utf8")));
        return new Map(records.binaries.map((record) => [record.target, record]));
      },
      checkConsumers: this.consumers,
      hostEnvironment: (): NodeJS.ProcessEnv => ({ ...process.env, PATH: this.globalBin }),
      callerSessionId: (): undefined => undefined,
    } satisfies Partial<RuntimeOperations>;
    let failure: Error | null = null;
    try {
      await runLifecycle(this.plan, overrides);
    } catch (error) {
      failure = error instanceof Error ? error : new Error(String(error));
    } finally {
      for (const owned of this.children) await releaseHarmlessChild(owned.child, owned.identity);
    }
    return failure;
  }
}
