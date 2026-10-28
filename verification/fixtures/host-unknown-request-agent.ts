import assert from "node:assert/strict";
import { requireArgument, wireJson } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const scenario = requireArgument(2);
const agent = new AcpExchange();
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: {},
  agentInfo: {
    name: scenario === "sessionless" ? "sessionless-request-fixture" : "unknown-request-fixture",
    version: "1",
  },
});
if (scenario !== "sessionless") await agent.newSession();
function unknownRequest(): void {
  agent.send({
    jsonrpc: "2.0",
    id: 91,
    method: scenario === "sessionless" ? "unknown/sessionless" : "session/unknown",
    params: scenario === "sessionless" ? {} : { sessionId: "fixture-session" },
  });
}
unknownRequest();
let response: Record<string, unknown> | undefined;
let prompt: Record<string, unknown> | undefined;
while (response === undefined || prompt === undefined) {
  const message = await agent.read();
  if (scenario === "sessionless" && message.method === "session/new")
    agent.respond(message, { sessionId: "fixture-session" });
  else if (message.method === "session/prompt") prompt = message;
  else if (message.id === 91n && Object.hasOwn(message, "error")) response = message;
  else
    assert.fail(
      scenario === "sessionless"
        ? "unexpected message while awaiting sessionless reply and prompt"
        : "unexpected message while awaiting unknown-request reply and prompt",
    );
}
let report: unknown = response;
if (scenario !== "sessionless") {
  unknownRequest();
  report = [response, await agent.read()];
}
agent.update({
  sessionUpdate: "agent_message_chunk",
  content: { type: "text", text: wireJson(report) },
});
agent.endTurn(prompt);
await agent.drain();
agent.close();
