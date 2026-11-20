import { AcpExchange } from "./acp-exchange.ts";
const agent = new AcpExchange();
await agent.initialize("closed-auth-sink-fixture");
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
