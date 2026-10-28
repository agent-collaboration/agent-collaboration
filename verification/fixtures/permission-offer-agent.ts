import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";

const mode = requireArgument(2);
const receipt = requireArgument(3);
const agent = new AcpExchange();
await agent.initialize("permission-fixture");
await agent.newSession();
const prompt = await agent.read("session/prompt");
const options =
  mode === "persistent"
    ? [{ optionId: "allow-always", name: "Always allow", kind: "allow_always" }]
    : [
        { optionId: "same", name: "Allow", kind: "allow_once" },
        { optionId: "same", name: "Reject", kind: "reject_once" },
      ];
agent.send({
  jsonrpc: "2.0",
  id: 91,
  method: "session/request_permission",
  params: {
    sessionId: "fixture-session",
    toolCall: {
      toolCallId: "tool-1",
      title: "Run command",
      kind: "execute",
      rawInput: { command: "echo safe", api_key: "secret" },
    },
    options,
  },
});
await agent.responseReceipt(receipt, 91n, true);
agent.endTurn(prompt);
await agent.drain();
agent.close();
