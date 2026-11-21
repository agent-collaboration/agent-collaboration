import { AcpExchange } from "./acp-exchange.ts";
const kind = process.argv[2] ?? "text";
const agent = new AcpExchange();
await agent.initialize(kind === "auth" ? "closed-auth-sink-fixture" : "closed-sink-fixture");
await agent.newSession();
const prompt = await agent.read("session/prompt");
if (kind === "auth")
  agent.send({
    jsonrpc: "2.0",
    method: "_auth/status_update",
    params: { authStatus: { kind: "none", label: "Signed out" } },
  });
else
  agent.update({ sessionUpdate: "agent_message_chunk", content: { type: "text", text: "Chunk" } });
agent.endTurn(prompt);
await agent.drain();
agent.close();
