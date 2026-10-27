import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";

const receipt = requireArgument(2);
const agent = new AcpExchange();
await agent.initialize("question-fixture");
await agent.newSession();
const prompt = await agent.read("session/prompt");
agent.update({
  sessionUpdate: "tool_call",
  toolCallId: "question-tool",
  title: "Ask",
  kind: "think",
  status: "pending",
});
agent.send({
  jsonrpc: "2.0",
  id: "question-request",
  method: "cursor/ask_question",
  params: {
    toolCallId: "question-tool",
    title: "Choose",
    questions: [
      {
        id: "single",
        prompt: "First?",
        options: [
          { id: "first-id", label: "Same" },
          { id: "second-id", label: "Same" },
        ],
      },
      {
        id: "multi",
        prompt: "Which?",
        allowMultiple: true,
        options: [
          { id: "alpha", label: "Alpha" },
          { id: "beta", label: "Beta" },
        ],
      },
    ],
  },
});
await agent.responseReceipt(receipt, "question-request");
agent.endTurn(prompt);
await agent.drain();
agent.close();
