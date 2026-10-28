import { AcpExchange } from "./acp-exchange.ts";

const agent = new AcpExchange();
const initialized = await agent.read("initialize");
agent.send({
  jsonrpc: "2.0",
  method: "_auth/status_update",
  params: {
    authStatus: {
      kind: "account",
      label: "Connected account",
      account: { email: "private@example.invalid" },
      vendor: { token: "private" },
    },
  },
});
agent.respond(initialized, {
  protocolVersion: 1,
  agentCapabilities: {},
  agentInfo: { name: "auth-fixture", version: "1" },
});
await agent.newSession();
const prompt = await agent.read("session/prompt");
agent.send({
  jsonrpc: "2.0",
  method: "_auth/status_update",
  params: { authStatus: { kind: "none", label: "Signed out" } },
});
agent.endTurn(prompt);
await agent.drain();
agent.close();
