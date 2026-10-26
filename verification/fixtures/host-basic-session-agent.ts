import assert from "node:assert/strict";
import { setTimeout } from "node:timers/promises";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";

const scenario = requireArgument(2);
const agent = new AcpExchange();
const names: Readonly<Record<string, string>> = {
  conversation: "conversation-fixture",
  cancel: "cancel-fixture",
  successor: "successor-fixture",
  permission: "permission-fixture",
  aggregate: "aggregate-fixture",
  replay: "load-fixture",
  mcp: "mcp-fixture",
};
const name = names[scenario];
if (name === undefined) throw new Error("unknown host session scenario");
const capabilities =
  scenario === "mcp"
    ? { loadSession: true, mcpCapabilities: { http: true } }
    : scenario === "conversation" || scenario === "replay"
      ? { loadSession: true }
      : {};
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: capabilities,
  agentInfo: { name, version: "1" },
});
if (scenario === "mcp") {
  const request = await agent.read(requireArgument(3));
  assert.deepEqual(field(request, "params", "mcpServers"), [
    { type: "http", name: "router-collaboration", url: "http://127.0.0.1:19090/mcp", headers: [] },
  ]);
  agent.respond(request, request.method === "session/new" ? { sessionId: "fixture-session" } : {});
} else if (scenario === "replay") {
  const loaded = await agent.read("session/load");
  agent.update({
    sessionUpdate: "agent_message_chunk",
    content: { type: "text", text: "OLD_HISTORY" },
  });
  agent.respond(loaded, {});
  const prompt = await agent.read("session/prompt");
  agent.update({
    sessionUpdate: "agent_message_chunk",
    content: { type: "text", text: "NEW_OUTPUT" },
  });
  agent.endTurn(prompt);
} else {
  agent.respond(await agent.read(scenario === "conversation" ? "session/new" : undefined), {
    sessionId: "fixture-session",
  });
  const prompt = await agent.read(scenario === "successor" ? undefined : "session/prompt");
  if (scenario === "cancel" || scenario === "aggregate") {
    if (scenario === "aggregate")
      for (let index = 0; index < 2; index++)
        agent.update({
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "x".repeat(600 * 1024) },
        });
    await agent.read("session/cancel");
    agent.endTurn(prompt, "cancelled");
  } else if (scenario === "permission") {
    agent.send({
      jsonrpc: "2.0",
      id: 91,
      method: "session/request_permission",
      params: {
        sessionId: "fixture-session",
        toolCall: { toolCallId: "permission-tool" },
        options: [{ optionId: "reject", name: "Reject", kind: "reject_once" }],
      },
    });
    const answer = await agent.read();
    assert.equal(answer.id, 91n);
    assert.equal(field(answer, "result", "outcome", "outcome"), "cancelled");
    agent.endTurn(prompt);
  } else {
    agent.endTurn(prompt);
    if (scenario === "successor") {
      const second = await agent.read();
      await setTimeout(150);
      agent.endTurn(second);
    }
  }
}
await agent.drain();
agent.close();
