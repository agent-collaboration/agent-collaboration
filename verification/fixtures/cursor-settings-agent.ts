import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { AcpStdio, requestParameters, requireArgument, wireJson } from "./acp-stdio.ts";

interface ConfigOption {
  readonly id: string;
  readonly name: string;
  readonly category: string;
  readonly type: "select" | "boolean";
  readonly currentValue: string | boolean;
  readonly options?: readonly { readonly value: string; readonly name: string }[];
}

function option(
  configId: string,
  category: string,
  values: readonly string[],
  currentValue: string,
): ConfigOption {
  return {
    id: configId,
    name: configId,
    category,
    type: "select",
    currentValue,
    options: values.map((value) => ({ value, name: value })),
  };
}

interface CursorSettings {
  mode: string;
  model: string;
  thinking: string;
  context: string;
  effort: string;
  fast: string;
}

function catalogOptions(
  scenario: string,
  current: CursorSettings,
  effortId: string,
): ConfigOption[] {
  let models = ["fixture-a", "fixture-b"];
  let efforts = ["medium", "high"];
  if (scenario === "dynamic" && current.model === "fixture-a") {
    efforts = ["medium"];
    if (current.mode === "agent") models = ["fixture-a"];
  }
  const result = [
    option("mode", "mode", ["agent", "ask"], current.mode),
    option("model", "model", models, current.model),
    option("thinking", "thought_level", ["false", "true"], current.thinking),
    option("context", "model_config", ["300k", "1m"], current.context),
  ];
  if (scenario !== "thinking-only")
    result.push(option(effortId, "thought_level", efforts, current.effort));
  if (["ambiguous", "explicit-wins", "duplicate-explicit"].includes(scenario))
    result.push(
      option(
        scenario === "duplicate-explicit" ? "effort" : "reasoning",
        "thought_level",
        ["medium", "high"],
        "medium",
      ),
    );
  if (scenario === "duplicate-mixed")
    result.push({
      id: "effort",
      name: "effort",
      category: "thought_level",
      type: "boolean",
      currentValue: true,
    });
  result.push(option("fast", "model_config", ["false", "true"], current.fast));
  return result;
}

async function serveSharedSettings(peer: {
  readonly current: CursorSettings;
  readonly readRequest: () => Promise<Record<string, unknown> | undefined>;
  readonly reply: (request: Record<string, unknown>, result: unknown) => void;
}): Promise<void> {
  const sessions = new Set<string>();
  for (;;) {
    const request = await peer.readRequest();
    if (request === undefined) return;
    if (request.method === "session/new") {
      const sessionId = `shared-session-${sessions.size + 1}`;
      sessions.add(sessionId);
      peer.reply(request, {
        sessionId,
        configOptions: catalogOptions("shared-settings", peer.current, "effort"),
      });
      continue;
    }
    assert.equal(request.method, "session/set_config_option");
    const parameters = requestParameters(request);
    assert.ok(typeof parameters.sessionId === "string" && sessions.has(parameters.sessionId));
    if (parameters.configId === "model") {
      assert.ok(parameters.value === "fixture-a" || parameters.value === "fixture-b");
      peer.current.model = parameters.value;
    } else {
      assert.equal(parameters.configId, "effort");
      assert.ok(parameters.value === "medium" || parameters.value === "high");
      peer.current.effort = parameters.value;
    }
    // Cursor's shared manager changes the value, without notifying the other session.
    peer.reply(request, {
      configOptions: catalogOptions("shared-settings", peer.current, "effort"),
    });
  }
}

export async function main(): Promise<void> {
  const scenario = requireArgument(2);
  const receiptPath = requireArgument(3);
  const requests: Record<string, unknown>[] = [];
  const current = {
    mode: "agent",
    model: "fixture-a",
    thinking: "true",
    context: "300k",
    effort: "medium",
    fast: "false",
  };
  let effortId: "effort" | "budget" = "effort";
  const io = new AcpStdio();
  async function readRequest(): Promise<Record<string, unknown> | undefined> {
    const request = await io.readRequest();
    if (request !== undefined) {
      requests.push(request);
      writeFileSync(receiptPath, wireJson(requests));
    }
    return request;
  }
  const reply = (request: Record<string, unknown>, result: unknown): void => {
    io.send({ jsonrpc: "2.0", id: request.id, result });
  };
  try {
    const initialize = await readRequest();
    assert.equal(initialize?.method, "initialize");
    if (initialize === undefined) throw new Error("initialize request required");
    reply(initialize, {
      protocolVersion: 1n,
      agentCapabilities: { loadSession: true, mcpCapabilities: { http: true } },
      agentInfo: { name: "cursor-settings-wire-peer", version: "1" },
    });
    if (scenario === "negotiation") {
      await io.drain();
      return;
    }
    if (scenario === "shared-settings")
      return await serveSharedSettings({ current, readRequest, reply });
    const create = await readRequest();
    assert.ok(create?.method === "session/new" || create?.method === "session/load");
    if (scenario === "fallback" || scenario === "ambiguous") effortId = "budget";
    if (create.method === "session/new")
      reply(create, {
        sessionId: "fixture-session",
        configOptions: catalogOptions(scenario, current, effortId),
      });
    else {
      reply(create, { configOptions: catalogOptions(scenario, current, effortId) });
      effortId = "budget";
      io.send({
        jsonrpc: "2.0",
        method: "session/update",
        params: {
          sessionId: "fixture-session",
          update: {
            sessionUpdate: "config_option_update",
            configOptions: catalogOptions(scenario, current, effortId),
          },
        },
      });
    }
    const expectedIds: readonly ("mode" | "model" | "effort" | "budget")[] =
      scenario === "dynamic" ? ["mode", "model", "effort"] : [effortId];
    for (const expectedId of expectedIds) {
      const request = await readRequest();
      if (request === undefined) return;
      assert.equal(request.method, "session/set_config_option");
      if (
        ["ambiguous", "thinking-only", "duplicate-explicit", "duplicate-mixed"].includes(scenario)
      ) {
        io.send({
          jsonrpc: "2.0",
          id: request.id,
          error: { code: -32602n, message: "unexpected mutation: ambiguous or absent effort" },
        });
        await io.drain();
        return;
      }
      const parameters = requestParameters(request);
      assert.equal(parameters.configId, expectedId);
      const expectedValue =
        expectedId === "mode" ? "ask" : expectedId === "model" ? "fixture-b" : "high";
      assert.equal(parameters.value, expectedValue);
      if (expectedId === "mode") current.mode = expectedValue;
      else if (expectedId === "model") current.model = expectedValue;
      else current.effort = expectedValue;
      if (scenario === "adjusted") {
        current.context = "1m";
        current.fast = "true";
      }
      reply(request, { configOptions: catalogOptions(scenario, current, effortId) });
    }
    await io.drain();
  } finally {
    io.close();
  }
}

if (import.meta.main)
  main().catch((error: unknown): void => {
    console.error(error);
    process.exitCode = 1;
  });
