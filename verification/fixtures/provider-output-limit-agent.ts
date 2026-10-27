import { AcpExchange } from "./acp-exchange.ts";
const agent = new AcpExchange();
await agent.initialize("output-limit-fixture");
await agent.newSession();
const prompt = await agent.read("session/prompt");
agent.update({
  sessionUpdate: "agent_message_chunk",
  content: { type: "text", text: "x".repeat(1048577) },
});
await agent.read("session/cancel");
agent.endTurn(prompt);
await agent.drain();
agent.close();
