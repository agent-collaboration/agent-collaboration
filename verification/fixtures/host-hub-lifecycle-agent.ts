import assert from "node:assert/strict";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { selectOption } from "./provider-settings-options.ts";
const mode = requireArgument(2);
const agent = new AcpExchange();
if (mode === "settings") {
  const current = new Map([
    ["mode", "default"],
    ["model", "a"],
  ]);
  const options = (): Record<string, unknown>[] => [
    selectOption({ id: "mode", current: current.get("mode") ?? "", values: ["default", "ask"] }),
    selectOption({ id: "model", current: current.get("model") ?? "", values: ["a", "b", "c"] }),
  ];
  await agent.initialize("hub-settings-fixture");
  await agent.newSession("fixture-session", { configOptions: options() });
  for (const [configId, value] of [
    ["model", "b"],
    ["mode", "ask"],
  ]) {
    if (configId === undefined || value === undefined) throw new Error("fixture setting absent");
    const request = await agent.read("session/set_config_option");
    assert.equal(field(request, "params", "configId"), configId);
    current.set(configId, value);
    agent.respond(request, { configOptions: options() });
  }
  current.set("model", "c");
  agent.update({ sessionUpdate: "config_option_update", configOptions: options() });
} else {
  await agent.initialize("hub-lifecycle-fixture", {
    agentCapabilities: { loadSession: true, sessionCapabilities: { resume: {} } },
  });
  const request = await agent.read();
  if (mode === "create") {
    assert.equal(request.method, "session/new");
    agent.respond(request, { sessionId: "fixture-session" });
  } else if (mode === "load") {
    assert.equal(request.method, "session/load");
    for (const [sessionUpdate, text] of [
      ["user_message_chunk", "Earlier input"],
      ["agent_message_chunk", "Earlier output"],
    ])
      agent.update({ sessionUpdate, content: { type: "text", text } });
    agent.respond(request, {});
  } else if (mode === "resume") {
    assert.equal(request.method, "session/resume");
    agent.respond(request, {});
  } else assert.fail(mode);
}
await agent.drain();
agent.close();
