import assert from "node:assert/strict";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { selectOption } from "./provider-settings-options.ts";
const receipt = requireArgument(2);
const agent = new AcpExchange();
await agent.initialize("settings-fixture");
const request = await agent.read("session/new");
const current = new Map([
  ["mode", "auto"],
  ["model", "a"],
  ["effort", "low"],
]);
function options(): Record<string, unknown>[] {
  return [
    selectOption({ id: "mode", current: current.get("mode") ?? "", values: ["auto", "ask"] }),
    selectOption({
      id: "model",
      current: current.get("model") ?? "",
      values: current.get("mode") === "auto" ? ["a"] : ["a", "b"],
    }),
    selectOption({
      id: "effort",
      current: current.get("effort") ?? "",
      values: ["low", "high"],
      category: "thought_level",
    }),
  ];
}
agent.respond(request, { sessionId: "fixture-session", configOptions: options() });
const seen: string[] = [];
for (const [id, value] of [
  ["mode", "ask"],
  ["model", "b"],
  ["effort", "high"],
]) {
  if (id === undefined || value === undefined) throw new Error("fixture setting absent");
  const request = await agent.read("session/set_config_option");
  assert.equal(field(request, "params", "configId"), id);
  assert.equal(field(request, "params", "value"), value);
  seen.push(id);
  current.set(id, value);
  agent.respond(request, { configOptions: options() });
}
const prompt = await agent.read("session/prompt");
seen.push("prompt");
agent.receipt(receipt, seen);
agent.endTurn(prompt);
await agent.drain();
agent.close();
