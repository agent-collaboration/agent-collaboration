import assert from "node:assert/strict";
import { AcpExchange } from "./acp-exchange.ts";
import { requireArgument } from "./acp-stdio.ts";
import { field } from "../exact-json.ts";

const kind = requireArgument(2);
assert.equal(process.env.CLAUDE_CODE_EXECUTABLE, requireArgument(4));
process.stderr.write(`PRIVATE_CANARY:${"x".repeat(131072)}`);
const agent = new AcpExchange();
await agent.initialize("stdio-fixture");
await agent.newSession("stdio-session");
const prompt = await agent.read("session/prompt");
assert.deepEqual(field(prompt, "params", "prompt"), [
  { type: "text", text: "Complete the SQLite proof.\nKeep this exact second line PRIVATE_CANARY." },
]);
agent.send({
  jsonrpc: "2.0",
  id: "callback",
  method: kind === "approval" ? "session/request_permission" : "elicitation/create",
  params:
    kind === "approval"
      ? {
          sessionId: "stdio-session",
          toolCall: {
            toolCallId: "private-tool",
            title: "PRIVATE_CANARY",
            kind: "execute",
            rawInput: { command: "PRIVATE_CANARY" },
          },
          options: [{ optionId: "allow", name: "Allow once", kind: "allow_once" }],
        }
      : {
          mode: "form",
          sessionId: "stdio-session",
          message: "PRIVATE_CANARY",
          requestedSchema: { type: "object", properties: { answer: { type: "string" } } },
        },
});
let seenCancel = false;
let seenCallback = false;
while (!(seenCancel && seenCallback)) {
  const message = await agent.read();
  if (message.method === "session/cancel") seenCancel = true;
  else {
    assert.equal(message.id, "callback");
    assert.deepEqual(
      message.result,
      kind === "approval" ? { outcome: { outcome: "cancelled" } } : { action: "cancel" },
    );
    seenCallback = true;
  }
}
agent.receipt(requireArgument(3), {
  pid: process.pid,
  fullInputMatched: true,
  callbackCancelled: seenCallback,
  cancelReceived: seenCancel,
});
agent.endTurn(prompt, "cancelled");
await agent.drain();
agent.close();
