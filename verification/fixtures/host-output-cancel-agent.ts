import assert from "node:assert/strict";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { FixtureControlSocket } from "./fixture-control-socket.ts";
const agent = new AcpExchange();
const control = new FixtureControlSocket(requireArgument(2));
try {
  await control.connected();
  agent.respond(await agent.read(), {
    protocolVersion: 1,
    agentCapabilities: {},
    agentInfo: { name: "output-limit-fixture", version: "1" },
  });
  agent.respond(await agent.read(), { sessionId: "fixture-session" });
  const first = await agent.read("session/prompt");
  agent.update({
    sessionUpdate: "agent_message_chunk",
    content: { type: "text", text: "x".repeat(1024 * 1024 + 1) },
  });
  await agent.read("session/cancel");
  agent.endTurn(first, "cancelled");
  agent.update({
    sessionUpdate: "agent_message_chunk",
    content: { type: "text", text: "LATE_OUTPUT" },
  });
  agent.send({
    jsonrpc: "2.0",
    id: 99,
    method: "probe/unknown",
    params: { sessionId: "fixture-session" },
  });
  const barrier = await agent.read();
  assert.equal(barrier.id, 99n);
  assert.ok(Object.hasOwn(barrier, "error"));
  control.send("r");
  const second = await agent.read("session/prompt");
  agent.update({
    sessionUpdate: "agent_message_chunk",
    content: { type: "text", text: "CURRENT_OUTPUT" },
  });
  agent.endTurn(second);
  await agent.drain();
} finally {
  control.close();
  agent.close();
}
