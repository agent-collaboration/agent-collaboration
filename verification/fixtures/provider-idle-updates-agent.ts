import { AcpExchange } from "./acp-exchange.ts";
const agent = new AcpExchange();
await agent.initialize("idle-update-fixture", { _meta: { steering: { supported: true } } });
await agent.newSession("fixture-session", {
  modes: {
    currentModeId: "auto",
    availableModes: [
      { id: "auto", name: "Auto" },
      { id: "ask", name: "Ask" },
    ],
  },
});
agent.update({ sessionUpdate: "current_mode_update", currentModeId: "ask" });
agent.update({
  sessionUpdate: "config_option_update",
  configOptions: [
    {
      id: "model",
      name: "Model",
      category: "model",
      type: "select",
      currentValue: "b",
      options: [
        { value: "a", name: "A" },
        { value: "b", name: "B" },
      ],
    },
  ],
});
agent.update({
  sessionUpdate: "future_idle_kind",
  content: { type: "text", text: "Future detail" },
});
const prompt = await agent.read("session/prompt");
agent.update({
  sessionUpdate: "tool_call",
  toolCallId: "late-tool",
  title: "Run check",
  kind: "execute",
  status: "pending",
});
agent.endTurn(prompt);
const steer = await agent.read("_session/steering");
agent.update({
  sessionUpdate: "tool_call_update",
  toolCallId: "late-tool",
  title: "Check passed",
  status: "completed",
});
agent.respond(steer, { outcome: "promptRequired" });
await agent.drain();
agent.close();
