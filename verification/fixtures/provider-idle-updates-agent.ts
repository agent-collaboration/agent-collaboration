import { AcpExchange } from "./acp-exchange.ts";
import { requireArgument } from "./acp-stdio.ts";
import { fileAppears, startReleaseBarrier } from "./release-barrier.ts";
const receipt = requireArgument(2);
const includeMalformedTool = process.argv[3] === "malformed-tool";
startReleaseBarrier(receipt);
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
if (includeMalformedTool) {
  // The transport can claim this ID before typed decoding rejects the missing title.
  agent.update({
    sessionUpdate: "tool_call",
    toolCallId: "malformed-late-tool",
    kind: "execute",
    status: "pending",
  });
}
agent.update({
  sessionUpdate: "tool_call",
  toolCallId: "late-tool",
  title: "Run check",
  kind: "execute",
  status: "pending",
});
agent.endTurn(prompt);
await fileAppears(`${receipt}.release-late`);
if (includeMalformedTool) {
  agent.update({
    sessionUpdate: "tool_call_update",
    toolCallId: "malformed-late-tool",
    title: "There was no retained item",
    status: "completed",
  });
}
agent.update({
  sessionUpdate: "tool_call_update",
  toolCallId: "late-tool",
  title: "Check passed",
  status: "completed",
});
await agent.drain();
agent.close();
