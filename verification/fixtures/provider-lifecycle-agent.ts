import assert from "node:assert/strict";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { releasePipeBarrier } from "./fixture-pipe-barrier.ts";
const mode = requireArgument(2);
const cwd = requireArgument(3);
const receipt = requireArgument(4);
const seen: string[] = [];
const agent = new AcpExchange();
await agent.initialize("lifecycle-fixture", {
  agentCapabilities: { sessionCapabilities: { list: {}, resume: {}, close: {} } },
});
const listed = await agent.read("session/list");
assert.equal(field(listed, "params", "cwd"), cwd);
seen.push("list");
agent.respond(listed, {
  sessions: [{ sessionId: "fixture-session", cwd, title: "Fixture session" }],
});
const resumed = await agent.read("session/resume");
seen.push("resume");
agent.respond(resumed, {});
const prompt = await agent.read("session/prompt");
seen.push("prompt");
if (mode === "running-close") {
  await agent.read("session/cancel");
  seen.push("cancel");
  agent.endTurn(prompt, "cancelled");
} else agent.endTurn(prompt);
if (mode === "aborted-close") {
  const next = await agent.read("session/prompt");
  seen.push("prompt");
  agent.endTurn(next);
}
const close = await agent.read("session/close");
seen.push("close");
if (mode === "held-close") releasePipeBarrier(`${receipt}.ready`, `${receipt}.go`);
agent.receipt(receipt, seen);
agent.respond(close, {});
await agent.drain();
agent.close();
