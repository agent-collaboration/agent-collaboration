import assert from "node:assert/strict";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { selectOption } from "./provider-settings-options.ts";
const mode = requireArgument(2);
const agent = new AcpExchange();
await agent.initialize("supervisor-settings", {
  agentCapabilities: { sessionCapabilities: mode === "invalid_no_close" ? {} : { close: {} } },
});
const current = new Map([
  ["mode", "auto"],
  ["model", "a"],
]);
const options = (): Record<string, unknown>[] => [
  selectOption({ id: "mode", current: current.get("mode") ?? "", values: ["auto", "ask"] }),
  selectOption({ id: "model", current: current.get("model") ?? "", values: ["a", "b"] }),
];
await agent.newSession("settings-session", { configOptions: options() });
if (mode.startsWith("invalid")) {
  if (mode === "invalid") agent.respond(await agent.read("session/close"), {});
} else {
  const request = await agent.read("session/set_config_option");
  assert.equal(field(request, "params", "configId"), "mode");
  if (mode === "partial_first")
    agent.send({
      jsonrpc: "2.0",
      id: request.id,
      error: { code: -32603, message: "private detail" },
    });
  else {
    current.set("mode", "ask");
    agent.respond(request, { configOptions: options() });
    const modelRequest = await agent.read("session/set_config_option");
    assert.equal(field(modelRequest, "params", "configId"), "model");
    if (mode === "partial")
      agent.send({
        jsonrpc: "2.0",
        id: modelRequest.id,
        error: { code: -32603, message: "private detail" },
      });
    else {
      current.set("model", "b");
      agent.respond(modelRequest, { configOptions: options() });
    }
  }
}
await agent.drain();
agent.close();
