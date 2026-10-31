import * as z from "zod/mini";
import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";

const methodNames = ["newSession", "loadSession", "resumeSession", "setSessionConfigOption"];
const methodSchema = z.enum(methodNames);
const scenarioSchema = z.object({ params: z.unknown(), methods: z.array(methodSchema) });

function freezeInput(value: unknown): void {
  if (typeof value !== "object" || value === null) return;
  for (const child of Object.values(value)) freezeInput(child);
  Object.freeze(value);
}

function recordSdkLaunch(options: Record<string, unknown>, sdkLaunches: unknown[]): void {
  if (process.env.ROUTER_ROUTE_FIXTURE_SPAWN !== "1") return;
  {
    // Literal pinned SDK string-versus-object rule; harmless Node replaces native Claude.
    const argument =
      typeof options.settings === "string" ? options.settings : JSON.stringify(options.settings);
    const child = spawnSync(
      process.execPath,
      [
        "--eval",
        "process.stdout.write(JSON.stringify(process.argv.slice(1)))",
        "--",
        "--settings",
        String(argument),
      ],
      { encoding: "utf8", timeout: 2000 },
    );
    if (child.status !== 0) throw new Error("Harmless SDK argv fixture failed");
    const argv: unknown = JSON.parse(child.stdout);
    sdkLaunches.push({
      argv,
      debugText: `Spawning Claude Code process: --settings ${String(argument)}`,
    });
  }
}

export function runAcp(): {
  agent: Record<string, unknown>;
  connection: { closed: Promise<void> };
} {
  const scenario = scenarioSchema.parse(JSON.parse(process.env.ROUTER_ROUTE_FIXTURE_CASE ?? ""));
  freezeInput(scenario.params);
  const before = JSON.stringify(scenario.params);
  const calls: unknown[] = [];
  const settingsPaths: string[] = [];
  const priorFilesAvailable: boolean[] = [];
  const sdkLaunches: unknown[] = [];
  const forbiddenAccesses: string[] = [];
  const query = new Proxy(
    {},
    {
      get(_target, key): undefined {
        if (["next", "interrupt", "reset", "setModel", "setEffort"].includes(String(key))) {
          forbiddenAccesses.push(String(key));
          throw new Error("Unexpected query reader/reset access");
        }
        return undefined;
      },
      set(): boolean {
        forbiddenAccesses.push("write");
        throw new Error("Unexpected query write");
      },
    },
  );
  const configOptions: unknown[] = [];
  const agent: Record<string, unknown> = {
    sessions: {
      fixture: {
        query,
        configOptions,
        models: { currentModelId: "sonnet", availableModels: [{ modelId: "sonnet" }] },
        modelInfos: [{ value: "sonnet", supportsEffort: true, supportedEffortLevels: ["low"] }],
      },
    },
    client: { sessionUpdate: async (): Promise<void> => {} },
    dispose: async (): Promise<void> => {},
  };
  for (const name of methodNames) {
    agent[name] = async function (
      this: Record<string, unknown>,
      params: unknown,
    ): Promise<unknown> {
      calls.push({ method: name, params, sameReceiver: this === agent });
      if (name !== "setSessionConfigOption") {
        const request = z.record(z.string(), z.unknown()).parse(params);
        const metadata = z.record(z.string(), z.unknown()).parse(request._meta);
        const claude = z.record(z.string(), z.unknown()).parse(metadata.claudeCode);
        const options = z.record(z.string(), z.unknown()).parse(claude.options);
        if (typeof options.settings === "string") settingsPaths.push(options.settings);
        priorFilesAvailable.push(settingsPaths.every((path) => existsSync(path)));
        recordSdkLaunch(options, sdkLaunches);
        if (process.env.ROUTER_ROUTE_FIXTURE_REJECT === name)
          throw new Error("Controlled delegate rejection");
      }
      return { sessionId: "fixture", configOptions, _meta: { vendorResult: "kept" } };
    };
  }
  setImmediate((): void => {
    void (async (): Promise<void> => {
      const results: unknown[] = [];
      for (const method of scenario.methods) {
        const invoke = agent[method];
        if (typeof invoke !== "function") throw new Error("Fixture method absent");
        try {
          const result: unknown = await Reflect.apply(invoke, agent, [scenario.params]);
          results.push({ method, result });
        } catch (error: unknown) {
          results.push({ method, error: error instanceof Error ? error.message : "unknown" });
        }
      }
      process.stdout.write(
        `${JSON.stringify({
          calls,
          priorFilesAvailable,
          sdkLaunches,
          results,
          forbiddenAccesses,
          inputUnchanged: before === JSON.stringify(scenario.params),
          ambientBaseUrl: process.env.ANTHROPIC_BASE_URL,
          ambientToken: process.env.ANTHROPIC_AUTH_TOKEN,
          ambientModelConfig: process.env.CLAUDE_MODEL_CONFIG,
          home: process.env.HOME,
        })}\n`,
      );
    })();
  });
  return {
    agent,
    connection: {
      closed: new Promise<void>((resolve): void => {
        process.stdin.once("end", resolve);
      }),
    },
  };
}
