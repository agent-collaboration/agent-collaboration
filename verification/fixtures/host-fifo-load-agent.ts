import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const mode = requireArgument(2);
const agent = new AcpExchange();
if (mode === "load") {
  const initialized = await agent.read("initialize");
  agent.respond(initialized, { protocolVersion: 1, agentCapabilities: { loadSession: true } });
  const request = await agent.readRequest();
  if (request !== undefined) {
    assert.equal(request.method, "session/load");
    assert.equal(field(request, "params", "sessionId"), "fixture-session");
    writeFileSync(requireArgument(3), "session/load");
    if (requireArgument(4) === "true")
      agent.send({
        jsonrpc: "2.0",
        id: request.id,
        error: { code: -32001, message: "fixture load refused" },
      });
    else agent.respond(request, {});
  }
  await agent.drain();
} else {
  await agent.initialize("fifo-close-fixture", {
    agentCapabilities: { loadSession: true, sessionCapabilities: { close: {} } },
  });
  await agent.newSession("fifo-session");
  for (;;) {
    const request = await agent.readRequest();
    if (request === undefined) break;
    if (request.method === "session/close") writeFileSync(requireArgument(3), "closed");
    else if (request.method === "session/load") writeFileSync(requireArgument(4), "loaded");
    else assert.fail(String(request.method));
    agent.respond(request, {});
  }
}
agent.close();
