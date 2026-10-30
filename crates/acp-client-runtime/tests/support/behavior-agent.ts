import { appendFileSync, writeFileSync } from "node:fs";
import { AcpExchange } from "../../../../verification/fixtures/acp-exchange.ts";
import { field } from "../../../../verification/exact-json.ts";
const receipt = process.argv[2];
if (!receipt) throw new Error("receipt path required");
const compactCase = process.argv[3] ?? "manual";
writeFileSync(receipt, "");
const agent = new AcpExchange();
await agent.initialize("behavior-fixture", {
  agentCapabilities: {
    loadSession: true,
    promptCapabilities: { image: true },
    _meta: { steering: { supported: true } },
  },
});
let sequence = 0;
let held: Record<string, unknown> | undefined;
let currentModel = "old";
let currentEffort = "low";
function settingsOptions(): unknown[] {
  return [
    {
      id: "model",
      name: "Model",
      category: "model",
      type: "select",
      currentValue: currentModel,
      options: ["old", "new"].map((value) => ({ value, name: value })),
    },
    {
      id: "effort",
      name: "Effort",
      category: "thought_level",
      type: "select",
      currentValue: currentEffort,
      options: (currentModel === "new" ? ["low", "high"] : ["low"]).map((value) => ({
        value,
        name: value,
      })),
    },
  ];
}
for (;;) {
  const request = await agent.readRequest();
  if (!request) break;
  const params = request.params;
  if (request.method === "session/new") {
    sequence += 1;
    const sessionId = `session-${sequence}`;
    agent.respond(request, {
      sessionId,
      ...(compactCase === "settings" ? { configOptions: settingsOptions() } : {}),
    });
    agent.update(
      {
        sessionUpdate: "available_commands_update",
        availableCommands: [{ name: "review", description: "Review (skill)" }],
      },
      sessionId,
    );
  } else if (request.method === "session/set_config_option" && compactCase === "settings") {
    appendFileSync(receipt, `${JSON.stringify(request)}\n`);
    const configId = field(params, "configId");
    const value = field(params, "value");
    if (configId === "model" && (value === "old" || value === "new")) currentModel = value;
    else if (
      configId === "effort" &&
      (value === "low" || (value === "high" && currentModel === "new"))
    )
      currentEffort = value;
    else throw new Error("unexpected or stale setting write");
    agent.respond(request, { configOptions: settingsOptions() });
  } else if (request.method === "session/prompt") {
    appendFileSync(receipt, `${JSON.stringify(request)}\n`);
    const sessionId = field(params, "sessionId");
    if (typeof sessionId !== "string") throw new Error("sessionId required");
    const prompt = field(params, "prompt");
    const first: unknown = Array.isArray(prompt) ? prompt[0] : undefined;
    const text = field(first, "text");
    if (text === "hold") {
      held = request;
      agent.update(
        { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "HOLDING" } },
        sessionId,
      );
      continue;
    }
    agent.update(
      { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "before" } },
      sessionId,
    );
    if (text === "semantic-error") {
      agent.update(
        {
          sessionUpdate: "tool_call",
          toolCallId: "semantic-tool",
          title: "Compact",
          kind: "think",
          status: "completed",
          _meta: { hint: "malformed" },
        },
        sessionId,
      );
    }
    if (text === "remove-skill")
      agent.update(
        { sessionUpdate: "available_commands_update", availableCommands: [] },
        sessionId,
      );
    if (text === "/compact" && compactCase !== "none") {
      const hint =
        compactCase === "auto" ? "automatic" : compactCase === "malformed" ? "malformed" : "manual";
      if (compactCase !== "terminal")
        agent.update(
          {
            sessionUpdate: "tool_call",
            toolCallId: "compact-tool",
            title: "Compact",
            kind: "think",
            status: "in_progress",
            _meta: { hint },
          },
          sessionId,
        );
      agent.update(
        {
          sessionUpdate: compactCase === "terminal" ? "tool_call" : "tool_call_update",
          toolCallId: "compact-tool",
          title: "Compact",
          kind: "other",
          status: compactCase === "failed" ? "failed" : "completed",
          ...(compactCase === "terminal" ? { _meta: { hint } } : {}),
        },
        sessionId,
      );
    }
    agent.update(
      { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "after" } },
      sessionId,
    );
    agent.endTurn(
      request,
      text === "/compact" && compactCase === "cancelled" ? "cancelled" : "end_turn",
    );
  } else if (request.method === "session/cancel" && held) {
    if (compactCase === "remove-on-cancel") {
      const sessionId = field(held.params, "sessionId");
      if (typeof sessionId !== "string") throw new Error("held sessionId required");
      agent.update(
        { sessionUpdate: "available_commands_update", availableCommands: [] },
        sessionId,
      );
    }
    agent.endTurn(held, "cancelled");
    held = undefined;
  } else if (request.method === "session/close") agent.respond(request, {});
  else if (request.id !== undefined) agent.respond(request, {});
}
