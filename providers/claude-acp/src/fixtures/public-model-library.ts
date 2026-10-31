import * as z from "zod/mini";
const requestSchema = z.object({ sessionId: z.string(), retired: z.optional(z.boolean()) });

function freshSession(query: object): {
  query: object;
  models: { currentModelId: string; availableModels: { modelId: string }[] };
  modelInfos: { value: string; supportsEffort: boolean; supportedEffortLevels: string[] }[];
  configOptions: { id: string; type: string; currentValue: string }[];
} {
  return {
    query,
    models: { currentModelId: "sonnet", availableModels: [{ modelId: "sonnet" }] },
    modelInfos: [{ value: "sonnet", supportsEffort: true, supportedEffortLevels: ["low", "high"] }],
    configOptions: [{ id: "model", type: "select", currentValue: "sonnet" }],
  };
}

export function runAcp(): {
  connection: { closed: Promise<void> };
  agent: Record<string, unknown>;
} {
  const sessions = { fixture: freshSession({}) };
  const calls: string[] = [];
  const agent: Record<string, unknown> = {
    sessions,
    client: { sessionUpdate: async (): Promise<void> => {} },
    dispose: async (): Promise<void> => {},
  };
  for (const name of ["newSession", "loadSession", "resumeSession", "setSessionConfigOption"]) {
    agent[name] = async (value: unknown): Promise<unknown> => {
      const params = requestSchema.parse(value);
      calls.push(name);
      if (params.sessionId !== "fixture") throw new Error("address mismatch");
      // Stock0.81 getOrCreateSession permits replacement; the response retains
      // exactly the installed Session's configOptions array (5841/5864/6478).
      const query = name === "resumeSession" ? sessions.fixture.query : {};
      sessions.fixture = freshSession(query);
      const result = { sessionId: params.sessionId, configOptions: sessions.fixture.configOptions };
      if (params.retired === true) {
        // Hold the old result across replacement, including same-query reuse.
        await Promise.resolve();
        sessions.fixture = freshSession(name === "resumeSession" ? query : {});
      }
      return result;
    };
  }
  setImmediate((): void => {
    void (async (): Promise<void> => {
      for (const retired of [false, true]) {
        for (const method of ["loadSession", "resumeSession"]) {
          const invoke = agent[method];
          if (typeof invoke !== "function") throw new Error("fixture method missing");
          const result: unknown = await Reflect.apply(invoke, agent, [
            { sessionId: "fixture", retired },
          ]);
          process.stdout.write(`${JSON.stringify({ method, retired, result })}\n`);
        }
      }
      process.stdout.write(`${JSON.stringify({ calls })}\n`);
    })();
  });
  const closed = new Promise<void>((resolve): void => {
    process.stdin.once("end", resolve);
  });
  return { connection: { closed }, agent };
}
