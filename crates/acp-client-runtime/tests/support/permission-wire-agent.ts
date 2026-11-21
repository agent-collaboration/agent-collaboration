import { AcpExchange } from "../../../../verification/fixtures/acp-exchange.ts";
import { requireArgument } from "../../../../verification/fixtures/acp-stdio.ts";

// Scripted agent for the N6 approval tap. It offers one persistent permission option. The receipt
// is the shared transport's complete wire capture: every line the agent received and every line it
// sent, in order, from `initialize` on, so the tap checks the whole exchange.
const receipt = requireArgument(3);
const mode = requireArgument(2);
const agent = new AcpExchange(undefined, { wireCapture: receipt });
await agent.initialize("permission-fixture");
await agent.newSession();
const prompt = await agent.read("session/prompt");
const permission = {
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
    options: [{ optionId: "allow-always", name: "Always allow", kind: "allow_always" }],
  },
};
if (mode === "duplicate-batch") {
  const sibling = {
    ...permission,
    id: 92,
    params: {
      ...permission.params,
      toolCall: { ...permission.params.toolCall, toolCallId: "tool-2" },
    },
  };
  agent.send([permission, permission, sibling]);
  await agent.nextLine();
  // Once the previous response was written, the same request ID can be reused.
  agent.send(permission);
} else {
  agent.send(permission);
}
await agent.read();
agent.endTurn(prompt);
if (mode === "two-turns") {
  const second = await agent.read("session/prompt");
  agent.send(permission);
  await agent.read();
  agent.endTurn(second);
}
await agent.drain();
agent.close();
