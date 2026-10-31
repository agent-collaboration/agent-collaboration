import * as z from "zod/mini";

const methodNames = ["newSession", "loadSession", "resumeSession", "setSessionConfigOption"];
const methodSchema = z.enum(methodNames);
const scenarioSchema = z.object({ params: z.unknown(), methods: z.array(methodSchema) });

function freezeInput(value: unknown): void {
  if (typeof value !== "object" || value === null) return;
  for (const child of Object.values(value)) freezeInput(child);
  Object.freeze(value);
}

export function runAcp(): {
  agent: Record<string, unknown>;
  connection: { closed: Promise<void> };
} {
  const scenario = scenarioSchema.parse(JSON.parse(process.env.ROUTER_ROUTE_FIXTURE_CASE ?? ""));
  freezeInput(scenario.params);
  const before = JSON.stringify(scenario.params);
  const calls: unknown[] = [];
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
