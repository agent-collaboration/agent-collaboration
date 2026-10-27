import assert from "node:assert/strict";
import { arrayField, field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const mode = requireArgument(2);
const agent = new AcpExchange();
await agent.initialize("multiblock-fixture", {
  agentCapabilities: mode === "image" ? { promptCapabilities: { image: true } } : {},
  _meta: mode === "steer" ? { steering: { supported: true } } : {},
});
await agent.newSession();
const request = await agent.read(mode === "steer" ? "_session/steering" : "session/prompt");
const contents = [
  { type: "text", text: "Read" },
  { type: "resource_link", uri: "file:///notes.txt", name: "notes.txt", mimeType: "text/plain" },
];
if (mode === "unsupported")
  assert.deepEqual(field(request, "params", "prompt"), [{ type: "text", text: "After rejection" }]);
else if (mode === "image")
  assert.deepEqual(field(request, "params", "prompt"), [
    { type: "image", data: "aGVsbG8=", mimeType: "image/png" },
  ]);
else if (mode === "approval") {
  assert.equal(arrayField(request, "params", "prompt").length, 2);
  agent.send({
    jsonrpc: "2.0",
    id: 91,
    method: "session/request_permission",
    params: {
      sessionId: "fixture-session",
      toolCall: { toolCallId: "tool-1", title: "Run command", kind: "execute" },
      options: [{ optionId: "allow-once", name: "Allow", kind: "allow_once" }],
    },
  });
  const answer = await agent.read();
  assert.equal(answer.id, 91n);
  assert.equal(field(answer, "result", "outcome", "outcome"), "cancelled");
} else assert.deepEqual(field(request, "params", "prompt"), contents);
if (mode === "steer") {
  assert.equal(field(request, "params", "_meta", "steering", "idleBehavior"), "promptRequired");
  agent.respond(request, { outcome: "promptRequired" });
} else agent.endTurn(request);
await agent.drain();
agent.close();
