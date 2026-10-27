import assert from "node:assert/strict";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { FixtureControlSocket } from "./fixture-control-socket.ts";
import { sendSocketNotice } from "./fixture-socket-notice.ts";
const scenario = requireArgument(2);
const agent = new AcpExchange();
const names: Readonly<Record<string, string>> = {
  two: "two-session-permission-fixture",
  cancel: "permission-cancellation-fixture",
  steer: "permission-steer-fixture",
  late: "late-permission-fixture",
  output: "output-limit-permission-fixture",
};
const name = names[scenario];
if (name === undefined) throw new Error("unknown approval scenario");
const initialized = await agent.read();
const admission =
  scenario === "steer"
    ? {
        protocolVersion: 1,
        agentCapabilities: {},
        _meta: { steering: { supported: true } },
        agentInfo: { name, version: "1" },
      }
    : { protocolVersion: 1, agentCapabilities: {}, agentInfo: { name, version: "1" } };
agent.respond(initialized, admission);
agent.respond(await agent.read(), { sessionId: "session-a" });
if (scenario === "two") agent.respond(await agent.read(), { sessionId: "session-b" });
const prompt = await agent.read(
  scenario === "two" || scenario === "late" || scenario === "output" ? "session/prompt" : undefined,
);
if (scenario === "two") assert.equal(field(prompt, "params", "sessionId"), "session-a");
if (scenario === "late") {
  await sendSocketNotice(requireArgument(3), "prompt");
  await agent.read("session/cancel");
}
const toolCall =
  scenario === "late"
    ? { toolCallId: "permission-after-cancel", title: "Late tool", kind: "execute" }
    : scenario === "output"
      ? { toolCallId: "pending-tool", title: "Pending tool", kind: "execute" }
      : { toolCallId: "permission-a", title: "Run an approved command", kind: "execute" };
agent.send({
  jsonrpc: "2.0",
  id: 91,
  method: "session/request_permission",
  params: {
    sessionId: "session-a",
    toolCall,
    options: [{ optionId: "allow-a", name: "Allow once", kind: "allow_once" }],
  },
});
if (scenario === "two") {
  const second = await agent.read("session/prompt");
  assert.equal(field(second, "params", "sessionId"), "session-b");
  agent.update(
    { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "session-b-replied" } },
    "session-b",
  );
  agent.endTurn(second);
} else if (scenario === "cancel" && requireArgument(3) === "peer")
  agent.send({ jsonrpc: "2.0", method: "$/cancel_request", params: { requestId: 91 } });
else if (scenario === "steer")
  agent.respond(await agent.read("_session/steering"), { outcome: "injected" });
else if (scenario === "output") {
  const gate = new FixtureControlSocket(requireArgument(3));
  try {
    await gate.connected();
    await gate.expect("g");
  } finally {
    gate.close();
  }
  agent.update(
    {
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "x".repeat(1024 * 1024 + 1) },
    },
    "session-a",
  );
}
const permission = await agent.read();
assert.equal(permission.id, 91n);
assert.equal(
  field(permission, "result", "outcome", "outcome"),
  scenario === "two" || scenario === "steer" ? "selected" : "cancelled",
);
if (scenario === "two") {
  assert.equal(field(permission, "result", "outcome", "optionId"), "allow-a");
  agent.send({ jsonrpc: "2.0", method: "$/cancel_request", params: { requestId: 91 } });
}
if (scenario === "output") await agent.read("session/cancel");
if (scenario === "two" || scenario === "cancel" || scenario === "steer")
  agent.update(
    {
      sessionUpdate: "agent_message_chunk",
      content: {
        type: "text",
        text:
          scenario === "two"
            ? "session-a-approved"
            : scenario === "cancel"
              ? "permission-cancelled"
              : "session-a-steered-and-approved",
      },
    },
    "session-a",
  );
agent.endTurn(prompt, scenario === "two" || scenario === "steer" ? "end_turn" : "cancelled");
await agent.drain();
agent.close();
