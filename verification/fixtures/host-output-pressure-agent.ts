import assert from "node:assert/strict";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { textField } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const scenario = requireArgument(2);
const agent = new AcpExchange();
const names: Readonly<Record<string, string>> = {
  replay: "replay-fixture",
  updates: "many-updates-fixture",
  frames: "large-frames-fixture",
  permission: "large-permission-fixture",
  limit: "frame-limit-fixture",
};
const name = names[scenario];
if (name === undefined) throw new Error("unknown pressure scenario");
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: scenario === "replay" ? { loadSession: true } : {},
  agentInfo: { name, version: "1" },
});
if (scenario === "frames") {
  for (const session of ["large-session", "second-session"]) await agent.newSession(session);
  const prompts = new Map<string, Record<string, unknown>>();
  for (let index = 0; index < 2; index++) {
    const request = await agent.read("session/prompt");
    prompts.set(textField(request, "params", "sessionId"), request);
  }
  const first = prompts.get("large-session");
  const second = prompts.get("second-session");
  if (first === undefined || second === undefined) throw new Error("both fixture prompts required");
  agent.update(
    {
      sessionUpdate: "tool_call",
      toolCallId: "large-tool",
      title: "large result",
      kind: "other",
      status: "in_progress",
    },
    "large-session",
  );
  const large = "x".repeat(2 * 1024 * 1024);
  agent.update(
    {
      sessionUpdate: "tool_call_update",
      toolCallId: "large-tool",
      rawOutput: { payload: large },
      status: "completed",
    },
    "large-session",
  );
  agent.respond(first, { stopReason: "end_turn", _meta: { payload: large } });
  agent.update(
    { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "SECOND_SESSION_OK" } },
    "second-session",
  );
  agent.endTurn(second);
} else if (scenario === "replay" || scenario === "updates") {
  const count = z.coerce.number().int().min(0).parse(requireArgument(3));
  const loaded = scenario === "replay" ? await agent.read("session/load") : undefined;
  if (loaded === undefined) agent.respond(await agent.read(), { sessionId: "fixture-session" });
  const prompt = loaded === undefined ? await agent.read("session/prompt") : undefined;
  for (let index = 0; index < count; index++)
    agent.update({
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: scenario === "replay" ? `historic-${index}` : "x" },
    });
  if (loaded !== undefined) agent.respond(loaded, {});
  const current = prompt ?? (await agent.read("session/prompt"));
  agent.update({
    sessionUpdate: "agent_message_chunk",
    content: { type: "text", text: scenario === "replay" ? "CURRENT_OUTPUT" : "DONE" },
  });
  agent.endTurn(current);
} else {
  agent.respond(await agent.read(), { sessionId: "fixture-session" });
  const prompt = await agent.read();
  if (scenario === "permission") {
    agent.send({
      jsonrpc: "2.0",
      id: 91,
      method: "session/request_permission",
      params: {
        sessionId: "fixture-session",
        toolCall: {
          toolCallId: "large-tool",
          title: "large permission",
          kind: "execute",
          rawInput: { payload: "x".repeat(2 * 1024 * 1024) },
        },
        options: [{ optionId: "reject", name: "Reject", kind: "reject_once" }],
      },
    });
    assert.equal((await agent.read()).id, 91n);
    agent.endTurn(prompt);
  } else {
    await setTimeout(100);
    agent.update({
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "x".repeat(64 * 1024 * 1024 + 1) },
    });
  }
}
await agent.drain();
agent.close();
