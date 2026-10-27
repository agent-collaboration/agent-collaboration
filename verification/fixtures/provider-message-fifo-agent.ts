import assert from "node:assert/strict";
import { appendFileSync } from "node:fs";
import { field } from "../exact-json.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { requireArgument } from "./acp-stdio.ts";
import { FixtureControlSocket } from "./fixture-control-socket.ts";
import { sendSocketNotice } from "./fixture-socket-notice.ts";

const mode = requireArgument(2);
const path = requireArgument(3);
const agent = new AcpExchange();
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: mode === "load" ? { loadSession: true } : {},
  agentInfo: {
    name:
      mode === "load"
        ? "load-fixture"
        : mode === "blocks"
          ? "queue-block-fixture"
          : "cursor-fixture",
    version: "1",
  },
});
async function gatedPrompt(notice: string): Promise<Record<string, unknown>> {
  const request = await agent.read("session/prompt");
  if (mode === "blocks")
    assert.deepEqual(
      field(request, "params", "prompt"),
      notice === "first"
        ? [{ type: "text", text: "first" }]
        : [
            { type: "resource_link", uri: "https://example.test/context", name: "context" },
            { type: "text", text: "queued text" },
          ],
    );
  const socket = new FixtureControlSocket(path);
  try {
    await socket.connected();
    socket.send(notice);
    await socket.waitForByte();
  } finally {
    socket.close();
  }
  return request;
}
if (mode === "load") {
  for (;;) {
    const request = await agent.readRequest();
    if (request === undefined) break;
    assert.equal(request.method, "session/load");
    assert.equal(field(request, "params", "sessionId"), "fixture-session");
    appendFileSync(path, "session/load\n");
    agent.send({
      jsonrpc: "2.0",
      id: request.id,
      error: { code: -32001, message: "refused load" },
    });
  }
} else {
  assert.ok(["cursor", "blocks", "refusal"].includes(mode));
  await agent.newSession();
  agent.endTurn(await gatedPrompt(mode === "refusal" ? "active" : "first"));
  if (mode === "refusal") {
    const refused = await agent.read("session/prompt");
    agent.send({
      jsonrpc: "2.0",
      id: refused.id,
      error: { code: -32000, message: "provider requires authentication" },
    });
    const second = await agent.read("session/prompt");
    await sendSocketNotice(path, "second");
    agent.endTurn(second);
  } else agent.endTurn(await gatedPrompt(mode === "blocks" ? "blocks-ok" : "second"));
  await agent.drain();
}
agent.close();
