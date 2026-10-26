import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const kind = requireArgument(2);
const receipt = requireArgument(3);
const agent = new AcpExchange();
await agent.initialize("interaction-order-fixture");
await agent.newSession();
const prompt = await agent.read("session/prompt");
if (kind === "question" || kind === "plan")
  agent.update({
    sessionUpdate: "tool_call",
    toolCallId: "interaction-tool",
    title: "Interaction",
    kind: "other",
    status: "pending",
  });
const method =
  kind === "form"
    ? "elicitation/create"
    : kind === "question"
      ? "cursor/ask_question"
      : "cursor/create_plan";
const params =
  kind === "form"
    ? {
        mode: "form",
        sessionId: "fixture-session",
        message: "Confirm",
        requestedSchema: { type: "object", properties: { answer: { type: "string" } } },
      }
    : kind === "question"
      ? {
          toolCallId: "interaction-tool",
          title: "Choose",
          questions: [{ id: "choice", prompt: "Proceed?", options: [{ id: "yes", label: "Yes" }] }],
        }
      : {
          toolCallId: "interaction-tool",
          name: "Plan",
          overview: "Overview",
          plan: "# Plan",
          todos: [{ id: "one", content: "Step", status: "pending" }],
        };
agent.send({ jsonrpc: "2.0", id: "interaction", method, params });
const first = await agent.read();
const second = await agent.read();
agent.receipt(receipt, [first, second]);
agent.endTurn(prompt, "cancelled");
await agent.drain();
agent.close();
