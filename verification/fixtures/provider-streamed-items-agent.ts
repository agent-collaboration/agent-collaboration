import { AcpExchange } from "./acp-exchange.ts";
const agent = new AcpExchange();
await agent.initialize("item-fixture");
await agent.newSession();
const prompt = await agent.read("session/prompt");
for (const [sessionUpdate, text] of [
  ["agent_message_chunk", "Hello"],
  ["agent_message_chunk", " world"],
  ["agent_thought_chunk", "Thinking"],
])
  agent.update({ sessionUpdate, content: { type: "text", text } });
for (const update of [
  {
    sessionUpdate: "tool_call",
    toolCallId: "tool-one",
    title: "Run check",
    kind: "execute",
    status: "pending",
  },
  {
    sessionUpdate: "tool_call_update",
    toolCallId: "tool-one",
    title: "Check passed",
    status: "completed",
  },
  {
    sessionUpdate: "plan",
    entries: [{ content: "Build client", priority: "medium", status: "pending" }],
  },
  { sessionUpdate: "future_kind", content: { type: "text", text: "Future detail" } },
  { sessionUpdate: "current_mode_update", currentModeId: "ask" },
  { sessionUpdate: "config_option_update", configOptions: [] },
  { sessionUpdate: "session_info_update", title: "Working session" },
  { sessionUpdate: "usage_update", used: 5, size: 100 },
  { sessionUpdate: "available_commands_update", availableCommands: [] },
])
  agent.update(update);
agent.endTurn(prompt);
await agent.drain();
agent.close();
