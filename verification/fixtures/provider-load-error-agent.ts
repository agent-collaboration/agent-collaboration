import assert from "node:assert/strict";
import { AcpExchange } from "./acp-exchange.ts";
import { requireArgument } from "./acp-stdio.ts";

const marker = requireArgument(2);
const agent = new AcpExchange();
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: { loadSession: true },
  agentInfo: { name: "error-fixture", version: "1" },
});
for (;;) {
  const request = await agent.readRequest();
  if (request === undefined || request.method === "exit") break;
  if (request.method === "cursor/list_available_models") {
    agent.send({
      jsonrpc: "2.0",
      id: request.id,
      error: { code: -32601, message: "Model discovery is unsupported by this load error fixture" },
    });
    continue;
  }
  if (request.method === "shutdown") agent.respond(request, {});
  else {
    assert.equal(request.method, "session/load");
    agent.send({
      jsonrpc: "2.0",
      id: request.id,
      error: { code: -32600, message: marker, data: { privateText: marker } },
    });
  }
}
agent.close();
