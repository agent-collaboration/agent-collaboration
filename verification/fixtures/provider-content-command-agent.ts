import assert from "node:assert/strict";
import { appendFileSync } from "node:fs";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const mode = requireArgument(2);
const agent = new AcpExchange();
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: {},
  agentInfo: { name: mode === "content" ? "content-fixture" : "cancel-fixture", version: "1" },
});
await agent.newSession();
const prompt = await agent.read("session/prompt");
if (mode === "content") {
  assert.deepEqual(field(prompt, "params", "prompt"), [
    { type: "text", text: "Read" },
    { type: "resource_link", uri: "https://example.test/context", name: "context" },
  ]);
  agent.endTurn(prompt);
} else {
  appendFileSync(requireArgument(3), "prompt\n");
  await agent.read("session/cancel");
  appendFileSync(requireArgument(3), "cancel\n");
  agent.endTurn(prompt, "cancelled");
}
await agent.drain();
agent.close();
