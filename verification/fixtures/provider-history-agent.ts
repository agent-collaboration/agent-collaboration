import assert from "node:assert/strict";
import { existsSync, writeFileSync } from "node:fs";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const scenario = requireArgument(2);
const agent = new AcpExchange();
await agent.initialize(
  scenario === "reset"
    ? "replay-fixture"
    : scenario === "items"
      ? "replay-items-fixture"
      : "replay-live-tool-fixture",
  { agentCapabilities: { loadSession: true } },
);
const request = scenario === "reset" ? await agent.readRequest() : await agent.read("session/load");
if (request !== undefined) {
  assert.equal(request.method, "session/load");
  if (scenario === "reset") {
    const marker = requireArgument(3);
    if (!existsSync(marker)) writeFileSync(`${marker}.early`, "load preceded replay reset");
    agent.respond(request, {
      configOptions: [
        {
          id: "model",
          name: "Model",
          type: "select",
          currentValue: "model-a",
          options: [{ value: "model-a", name: "Model A" }],
        },
      ],
    });
  } else if (scenario === "items") {
    for (const [sessionUpdate, text] of [
      ["user_message_chunk", "First user"],
      ["agent_message_chunk", "First answer"],
      ["user_message_chunk", "Second user"],
      ["agent_message_chunk", "Second answer"],
    ])
      agent.update({ sessionUpdate, content: { type: "text", text } });
    agent.respond(request, {});
  } else {
    agent.update({
      sessionUpdate: "tool_call",
      toolCallId: "replayed-tool",
      title: "Old tool",
      kind: "execute",
      status: "pending",
    });
    agent.update({ sessionUpdate: "current_mode_update", currentModeId: "ask" });
    agent.respond(request, {
      modes: {
        currentModeId: "auto",
        availableModes: [
          { id: "auto", name: "Auto" },
          { id: "ask", name: "Ask" },
        ],
      },
    });
    const prompt = await agent.read("session/prompt");
    agent.update({
      sessionUpdate: "tool_call_update",
      toolCallId: "replayed-tool",
      title: "Tool finished",
      status: "completed",
    });
    agent.endTurn(prompt);
  }
}
await agent.drain();
agent.close();
