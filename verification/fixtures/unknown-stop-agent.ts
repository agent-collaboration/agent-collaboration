import { AcpExchange } from "./acp-exchange.ts";

const agent = new AcpExchange();
await agent.initialize("unknown-stop-fixture");
await agent.newSession();
const prompt = await agent.read("session/prompt");
for (const text of ["first ", "second"])
  agent.update({ sessionUpdate: "agent_message_chunk", content: { type: "text", text } });
agent.endTurn(prompt, "future_reason");
await agent.drain();
agent.close();
