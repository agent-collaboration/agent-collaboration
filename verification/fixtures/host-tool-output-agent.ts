import { z } from "zod";
import { decodeEvidenceJson } from "../evidence-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const rawOutput = decodeEvidenceJson(requireArgument(2));
const responseText = requireArgument(3);
const split = z.enum(["true", "false"]).parse(requireArgument(4)) === "true";
const agent = new AcpExchange();
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: {},
  agentInfo: { name: "tool-call-fixture", version: "1" },
});
agent.respond(await agent.read(), { sessionId: "fixture-session" });
const prompt = await agent.read();
agent.update({
  sessionUpdate: "tool_call",
  toolCallId: "call-1",
  title: "router-collaboration: endpoints_list",
  kind: "other",
});
if (rawOutput !== null && split)
  agent.update({
    sessionUpdate: "tool_call_update",
    toolCallId: "call-1",
    name: "router-collaboration-endpoints_list",
    rawOutput,
  });
const terminal: Record<string, unknown> = {
  sessionUpdate: "tool_call_update",
  toolCallId: "call-1",
  name: "router-collaboration-endpoints_list",
  status: "completed",
};
if (rawOutput !== null && !split) terminal.rawOutput = rawOutput;
agent.update(terminal);
if (responseText !== "")
  agent.update({
    sessionUpdate: "agent_message_chunk",
    content: { type: "text", text: responseText },
  });
agent.endTurn(prompt);
await agent.drain();
agent.close();
