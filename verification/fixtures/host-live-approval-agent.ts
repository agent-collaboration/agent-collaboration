import assert from "node:assert/strict";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const scenario = requireArgument(2);
const provider = process.argv[3] ?? "";
const agent = new AcpExchange();
const name =
  scenario === "owned"
    ? "owned-result-fixture"
    : scenario === "reject"
      ? "permission-refusal-fixture"
      : `${provider}-permission-fixture`;
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: {},
  agentInfo: { name, version: "1" },
});
if (scenario !== "owned") {
  const sessionId =
    scenario === "reject" ? "fixture-refusal-session" : `fixture-${provider}-session`;
  agent.respond(await agent.read(), { sessionId });
  const prompt = await agent.read(scenario === "reject" ? undefined : "session/prompt");
  const id = scenario === "reject" ? 92 : 91;
  const toolCall =
    scenario === "reject"
      ? { toolCallId: "refused-command", title: "Run an unapproved command", kind: "execute" }
      : { toolCallId: "echo-command", title: "Run echo cursor-ok", kind: "execute" };
  const option =
    scenario === "reject"
      ? { optionId: "reject", name: "Reject once", kind: "reject_once" }
      : { optionId: "allow", name: "Allow once", kind: "allow_once" };
  agent.send({
    jsonrpc: "2.0",
    id,
    method: "session/request_permission",
    params: { sessionId, toolCall, options: [option] },
  });
  const response = await agent.read();
  assert.equal(response.id, BigInt(id));
  assert.equal(field(response, "result", "outcome", "outcome"), "selected");
  assert.equal(field(response, "result", "outcome", "optionId"), option.optionId);
  if (scenario !== "reject")
    agent.update(
      { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "cursor-ok" } },
      sessionId,
    );
  agent.endTurn(prompt, scenario === "reject" ? "cancelled" : "end_turn");
}
await agent.drain();
agent.close();
