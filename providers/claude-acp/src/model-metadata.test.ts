import assert from "node:assert/strict";
import * as z from "zod/mini";
import { test } from "node:test";
import {
  ModelMetadataDecorator,
  decorateSessionMethods,
  modelReasoningSchema,
} from "./model-metadata.ts";
import type { ModelMetadataSession } from "./model-metadata.ts";

function session(): ModelMetadataSession {
  return {
    configOptions: [],
    sessionIdentity: {},
    queryIdentity: {},
    models: {
      currentModelId: "sonnet",
      availableModels: [{ modelId: "sonnet" }, { modelId: "haiku" }],
    },
    modelInfos: [
      { value: "sonnet", supportsEffort: true, supportedEffortLevels: ["low", "high"] },
      { value: "haiku", supportsEffort: false },
    ],
  };
}
const responseSchema = z.looseObject({
  _meta: z.looseObject({ codexRouter: z.looseObject({ modelReasoning: modelReasoningSchema }) }),
});
function resultSchema(value: unknown): z.infer<typeof modelReasoningSchema> {
  return responseSchema.parse(value)._meta.codexRouter.modelReasoning;
}

await test("initial and setter response metadata keeps every model and stock legacy reset", (): void => {
  const current = session();
  const decorator = new ModelMetadataDecorator({ currentSession: () => current });
  for (const response of [
    { sessionId: "owned", _meta: { vendor: "kept", codexRouter: { skills: "kept" } } },
    {},
  ]) {
    const result = decorator.decorateResult({
      sessionId: "owned",
      result: { ...response, configOptions: current.configOptions },
      source: { kind: "setting", before: current },
    });
    assert.deepEqual(resultSchema(result), {
      version: 1,
      kind: "available",
      models: [
        {
          modelId: "sonnet",
          kind: "select",
          configId: "effort",
          defaultValue: "default",
          choices: [
            { value: "default", label: "Default" },
            { value: "low", label: "Low" },
            { value: "high", label: "High" },
          ],
        },
        { modelId: "haiku", kind: "unknown", reason: "Unavailable" },
      ],
    });
  }
});

await test("retired query, foreign response and malformed models cannot publish qualified facts", (): void => {
  const before = session();
  let current = session();
  const decorator = new ModelMetadataDecorator({ currentSession: () => current });
  assert.deepEqual(
    resultSchema(
      decorator.decorateResult({
        sessionId: "owned",
        result: { configOptions: before.configOptions },
        source: { kind: "setting", before },
      }),
    ),
    {
      version: 1,
      kind: "unknown",
      reason: "Unavailable",
    },
  );
  const foreign = { sessionId: "foreign" };
  assert.equal(
    decorator.decorateResult({
      sessionId: "owned",
      result: foreign,
      source: { kind: "setting", before: current },
    }),
    foreign,
  );
  current = {
    ...current,
    modelInfos: [
      { value: "sonnet", supportsEffort: true, supportedEffortLevels: ["high", "high"] },
    ],
  };
  const facts = resultSchema(
    decorator.decorateResult({
      sessionId: "owned",
      result: { configOptions: current.configOptions },
      source: { kind: "setting", before: current },
    }),
  );
  assert.equal(facts.kind, "available");
  if (facts.kind === "available")
    assert.deepEqual(facts.models[0], { modelId: "sonnet", kind: "unknown", reason: "Ambiguous" });
  current = { ...current, modelInfos: "malformed" };
  assert.deepEqual(
    resultSchema(
      decorator.decorateResult({
        sessionId: "owned",
        result: { configOptions: current.configOptions },
        source: { kind: "setting", before: current },
      }),
    ),
    {
      version: 1,
      kind: "unknown",
      reason: "Malformed",
    },
  );
});

await test("config updates preserve other metadata while skill notifications stay identical", (): void => {
  const decorator = new ModelMetadataDecorator({ currentSession: () => session() });
  const skill = {
    sessionId: "owned",
    update: {
      sessionUpdate: "available_commands_update",
      _meta: { codexRouter: { skills: "unchanged" } },
    },
  };
  assert.equal(decorator.decorateNotification(skill), skill);
  const update = decorator.decorateNotification({
    sessionId: "owned",
    update: {
      sessionUpdate: "config_option_update",
      configOptions: [],
      _meta: { vendor: "kept", codexRouter: { skills: "unchanged" } },
    },
  });
  assert.match(JSON.stringify(update), /"vendor":"kept"/);
  assert.match(JSON.stringify(update), /"skills":"unchanged"/);
  assert.match(JSON.stringify(update), /"modelReasoning"/);
});

await test("all four result decorators delegate once and suppress facts across query retirement", async (): Promise<void> => {
  let current = session();
  const decorator = new ModelMetadataDecorator({ currentSession: () => current });
  const calls: string[] = [];
  const agent: Record<string, unknown> = {};
  for (const method of ["newSession", "loadSession", "resumeSession", "setSessionConfigOption"]) {
    agent[method] = async (params: unknown): Promise<unknown> => {
      calls.push(method);
      assert.deepEqual(params, { sessionId: "owned" });
      return {
        ...(method === "newSession" ? { sessionId: "owned" } : {}),
        configOptions: current.configOptions,
      };
    };
  }
  decorateSessionMethods(agent, decorator, (params) => ({ kind: "prepared", params }));
  for (const method of ["newSession", "loadSession", "resumeSession", "setSessionConfigOption"]) {
    const invoke = agent[method];
    if (typeof invoke !== "function") throw new Error("decorated method missing");
    const result: unknown = await Reflect.apply(invoke, agent, [{ sessionId: "owned" }]);
    assert.equal(resultSchema(result).kind, "available");
  }
  assert.deepEqual(calls, ["newSession", "loadSession", "resumeSession", "setSessionConfigOption"]);
  let release: (() => void) | undefined;
  const held = new Promise<void>((resolve): void => {
    release = resolve;
  });
  const delayed: Record<string, unknown> = {
    ...agent,
    setSessionConfigOption: async (): Promise<unknown> => {
      await held;
      return {};
    },
  };
  decorateSessionMethods(delayed, decorator, (params) => ({ kind: "prepared", params }));
  const invoke = delayed.setSessionConfigOption;
  if (typeof invoke !== "function") throw new Error("setter missing");
  const pending: unknown = Reflect.apply(invoke, delayed, [{ sessionId: "owned" }]);
  current = { ...current, queryIdentity: {} };
  if (release === undefined) throw new Error("barrier missing");
  release();
  assert.deepEqual(resultSchema(await pending), {
    version: 1,
    kind: "unknown",
    reason: "Unavailable",
  });
});

await test("producer rejects actual NUL and accepts a literal backslash followed by zero", (): void => {
  const model = (modelId: string): unknown => ({
    version: 1,
    kind: "available",
    models: [{ modelId, kind: "unknown", reason: "Unavailable" }],
  });
  assert.equal(modelReasoningSchema.safeParse(model("nul\u0000id")).success, false);
  assert.equal(modelReasoningSchema.safeParse(model("literal\\0id")).success, true);
});

await test("fresh load replacement qualifies its returned public options while superseded output stays unknown", async (): Promise<void> => {
  const initial = session();
  let current = initial;
  const decorator = new ModelMetadataDecorator({ currentSession: () => current });
  const agent: Record<string, unknown> = {};
  for (const name of ["newSession", "loadSession", "resumeSession", "setSessionConfigOption"]) {
    agent[name] = async (): Promise<unknown> => {
      current = session();
      return { sessionId: "owned", configOptions: current.configOptions };
    };
  }
  decorateSessionMethods(agent, decorator, (params) => ({ kind: "prepared", params }));
  for (const name of ["loadSession", "resumeSession"]) {
    const invoke = agent[name];
    if (typeof invoke !== "function") throw new Error("lifecycle method missing");
    const result: unknown = await Reflect.apply(invoke, agent, [{ sessionId: "owned" }]);
    assert.equal(resultSchema(result).kind, "available");
  }
});
