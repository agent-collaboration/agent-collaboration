import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { selectOption } from "./provider-settings-options.ts";
const mode = requireArgument(2);
const receipt = requireArgument(3);
const agent = new AcpExchange();
await agent.initialize("settings-failure-fixture", {
  agentCapabilities: { sessionCapabilities: { close: {} } },
});
const options = [
  selectOption({ id: "mode", current: "auto", values: ["auto", "ask"] }),
  selectOption({ id: "model", current: "a", values: ["a", "b"] }),
];
await agent.newSession("fixture-session", { configOptions: options });
if (mode === "invalid") {
  const close = await agent.read("session/close");
  writeFileSync(receipt, "closed");
  agent.respond(close, {});
} else {
  const setMode = await agent.read("session/set_config_option");
  assert.equal(field(setMode, "params", "configId"), "mode");
  options[0] = selectOption({ id: "mode", current: "ask", values: ["auto", "ask"] });
  agent.respond(setMode, { configOptions: options });
  const setModel = await agent.read("session/set_config_option");
  assert.equal(field(setModel, "params", "configId"), "model");
  agent.send({
    jsonrpc: "2.0",
    id: setModel.id,
    error: { code: -32603, message: "private agent detail" },
  });
  if (mode === "partial-set") {
    const retry = await agent.read("session/set_config_option");
    assert.equal(field(retry, "params", "configId"), "model");
    assert.equal(field(retry, "params", "value"), "b");
    options[1] = selectOption({ id: "model", current: "b", values: ["a", "b"] });
    agent.respond(retry, { configOptions: options });
  }
  const prompt = await agent.read("session/prompt");
  writeFileSync(receipt, "prompt-after-accept");
  agent.endTurn(prompt);
}
await agent.drain();
agent.close();
