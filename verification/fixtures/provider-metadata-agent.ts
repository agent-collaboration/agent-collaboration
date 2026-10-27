import assert from "node:assert/strict";
import { AcpExchange } from "./acp-exchange.ts";
const agent = new AcpExchange();
await agent.initialize("metadata-fixture", { agentCapabilities: { loadSession: true } });
let request = await agent.read();
const replay = request.method === "session/load";
if (!replay) {
  assert.equal(request.method, "session/new");
  agent.respond(request, { sessionId: "metadata-session" });
  request = await agent.read("session/prompt");
}
for (const update of [
  {
    sessionUpdate: "available_commands_update",
    availableCommands: [
      { name: "compact", description: "Compact context", input: { hint: "instructions" } },
    ],
    _meta: { providerDetail: "retained" },
  },
  {
    sessionUpdate: "usage_update",
    used: 1234,
    size: 200000,
    cost: { amount: 0.25, currency: "USD" },
    _meta: { providerDetail: "usage-retained" },
  },
  { sessionUpdate: "current_mode_update", currentModeId: "default" },
  { sessionUpdate: "config_option_update", configOptions: [] },
  { sessionUpdate: "session_info_update", title: "Metadata example" },
  {
    sessionUpdate: "agent_message_chunk",
    content: { type: "text", text: "Ordinary assistant reply" },
  },
  { sessionUpdate: "agent_thought_chunk", content: { type: "text", text: "Useful reasoning" } },
])
  agent.update(update, "metadata-session");
agent.respond(request, replay ? {} : { stopReason: "end_turn" });
await agent.drain();
agent.close();
