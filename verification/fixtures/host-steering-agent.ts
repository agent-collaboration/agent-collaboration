import assert from "node:assert/strict";
import { sendSocketNotice } from "./fixture-socket-notice.ts";
import { arrayField, field, textField } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const agent = new AcpExchange();
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: { loadSession: true },
  agentInfo: { name: "steering-fixture", version: "1" },
  _meta: { steering: { supported: true } },
});
await agent.newSession();
const prompt = await agent.read("session/prompt");
await sendSocketNotice(requireArgument(2), "prompt");
const steer = await agent.read("_session/steering");
assert.equal(field(steer, "params", "sessionId"), "fixture-session");
assert.equal(textField(arrayField(steer, "params", "prompt")[0], "text"), "follow-up");
assert.equal(field(steer, "params", "_meta", "steering", "idleBehavior"), "promptRequired");
agent.respond(steer, { outcome: "injected" });
agent.endTurn(prompt);
agent.respond(await agent.read("_session/steering"), {
  outcome: "promptRequired",
  reason: "noRunningTurn",
});
await agent.drain();
agent.close();
