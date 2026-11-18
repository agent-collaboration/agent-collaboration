import assert from "node:assert/strict";
import { textField } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const scenario = requireArgument(2);
const agent = new AcpExchange();
await agent.initialize(scenario === "single" ? "single-todo-fixture" : "todo-fixture");
async function requireResponse(id: string, result: unknown): Promise<void> {
  const response = await agent.read();
  assert.equal(response.id, id);
  assert.deepEqual(response.result, result);
}
if (scenario === "late") {
  // Turn one announces a tool call and ends; in turn two a late todo update
  // still names it (review F4): the client must drop it.
  await agent.newSession("late-session");
  const first = await agent.read("session/prompt");
  agent.update(
    {
      sessionUpdate: "tool_call",
      toolCallId: "early-tool",
      title: "Update plan",
      kind: "other",
      status: "pending",
    },
    "late-session",
  );
  agent.endTurn(first);
  const second = await agent.read("session/prompt");
  agent.send({
    jsonrpc: "2.0",
    id: "late-todo",
    method: "cursor/update_todos",
    params: {
      toolCallId: "early-tool",
      todos: [{ id: "late", content: "Late task", status: "pending" }],
      merge: false,
    },
  });
  await requireResponse("late-todo", {});
  agent.endTurn(second);
} else if (scenario === "single") {
  await agent.newSession("only-session");
  const prompt = await agent.read("session/prompt");
  agent.send({
    jsonrpc: "2.0",
    id: "todo",
    method: "cursor/update_todos",
    params: {
      toolCallId: "unobserved",
      todos: [{ id: "one", content: "Fallback task", status: "pending" }],
      merge: false,
    },
  });
  await requireResponse("todo", {});
  agent.endTurn(prompt);
} else {
  for (const session of ["first", "second"]) await agent.newSession(session);
  const prompts = new Map<string, unknown>();
  for (let index = 0; index < 2; index++) {
    const prompt = await agent.read("session/prompt");
    prompts.set(textField(prompt, "params", "sessionId"), prompt.id);
  }
  for (const [sessionId, toolCallId, content] of [
    ["first", "tool-first", "First task"],
    ["second", "tool-second", "Second task"],
  ]) {
    if (sessionId === undefined) throw new Error("fixture session missing");
    agent.update(
      {
        sessionUpdate: "tool_call",
        toolCallId,
        title: "Update plan",
        kind: "other",
        status: "pending",
      },
      sessionId,
    );
    agent.send({
      jsonrpc: "2.0",
      id: `todo-${sessionId}`,
      method: "cursor/update_todos",
      params: { toolCallId, todos: [{ id: "one", content, status: "pending" }], merge: false },
    });
    await requireResponse(`todo-${sessionId}`, {});
    agent.send({
      jsonrpc: "2.0",
      id: `plan-${sessionId}`,
      method: "cursor/create_plan",
      params: {
        toolCallId,
        name: `Plan for ${sessionId}`,
        overview: "Scoped plan",
        plan: "# Steps",
        todos: [{ id: "one", content, status: "completed" }],
      },
    });
    await requireResponse(`plan-${sessionId}`, { outcome: "cancelled" });
  }
  agent.send({
    jsonrpc: "2.0",
    id: "todo-unknown",
    method: "cursor/update_todos",
    params: {
      toolCallId: "unknown",
      todos: [{ id: "x", content: "Must not appear", status: "pending" }],
      merge: false,
    },
  });
  await requireResponse("todo-unknown", {});
  agent.send({
    jsonrpc: "2.0",
    id: "question-unknown",
    method: "cursor/ask_question",
    params: {
      toolCallId: "unknown",
      questions: [{ id: "q", prompt: "Which?", options: [{ id: "a", label: "A" }] }],
    },
  });
  await requireResponse("question-unknown", { outcome: { outcome: "cancelled" } });
  for (const id of prompts.values()) agent.endTurn({ id });
}
await agent.drain();
agent.close();
