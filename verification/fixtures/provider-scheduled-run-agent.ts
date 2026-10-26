import assert from "node:assert/strict";
import { field } from "../exact-json.ts";
import { requireArgument, wireJson } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { FixtureControlSocket } from "./fixture-control-socket.ts";
const agent = new AcpExchange();
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: { loadSession: true },
  agentInfo: { name: "schedule-fixture", version: "1" },
});
await agent.newSession("scheduled-session");
const active = await agent.read("session/prompt");
const firstEvent = new FixtureControlSocket(requireArgument(2));
try {
  await firstEvent.connected();
  firstEvent.send("active");
  await firstEvent.waitForByte();
} finally {
  firstEvent.close();
}
agent.endTurn(active);
const scheduled = await agent.read("session/prompt");
assert.deepEqual(field(scheduled, "params", "prompt"), [
  { type: "text", text: requireArgument(3) },
]);
const secondEvent = new FixtureControlSocket(requireArgument(2));
try {
  await secondEvent.connected();
  secondEvent.send(`${wireJson(field(scheduled, "params"))}\n`);
  await secondEvent.waitForByte();
} finally {
  secondEvent.close();
}
agent.update(
  {
    sessionUpdate: "agent_message_chunk",
    content: { type: "text", text: "provider summary text" },
  },
  "scheduled-session",
);
agent.endTurn(scheduled);
const cancelled = await agent.read("session/prompt");
await agent.read("session/cancel");
agent.endTurn(cancelled, "cancelled");
agent.endTurn(await agent.read("session/prompt"), "refusal");
for (let index = 0; index < 256; index++) agent.endTurn(await agent.read("session/prompt"));
await agent.read("session/prompt");
await agent.read("session/cancel");
await agent.drain();
agent.close();
