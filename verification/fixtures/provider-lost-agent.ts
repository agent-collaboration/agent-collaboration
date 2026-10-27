import { closeSync } from "node:fs";
import { AcpExchange } from "./acp-exchange.ts";
const agent = new AcpExchange();
await agent.initialize("lost-fixture");
await agent.newSession();
await agent.read("session/prompt");
closeSync(1);
await agent.drain();
agent.close();
