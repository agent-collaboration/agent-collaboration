import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";

const receipt = requireArgument(2);
const agent = new AcpExchange();
await agent.initialize("plan-fixture");
await agent.newSession();
const prompt = await agent.read("session/prompt");
agent.update({
  sessionUpdate: "tool_call",
  toolCallId: "plan-tool",
  title: "Create plan",
  kind: "other",
  status: "pending",
});
agent.send({
  jsonrpc: "2.0",
  id: "create-plan",
  method: "cursor/create_plan",
  params: {
    toolCallId: "plan-tool",
    name: "Migration plan",
    overview: "Move clients safely",
    plan: "# Steps\n\nShip in slices.",
    todos: [{ id: "one", content: "Build client", status: "pending" }],
  },
});
await agent.responseReceipt(receipt, "create-plan");
agent.endTurn(prompt);
await agent.drain();
agent.close();
