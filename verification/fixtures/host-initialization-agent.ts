import { writeFileSync } from "node:fs";
import { z } from "zod";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const version = z.coerce.number().int().min(0).max(65535).parse(requireArgument(2));
const processIdPath = process.argv[3];
if (processIdPath !== undefined && processIdPath !== "")
  writeFileSync(processIdPath, String(process.pid));
const agent = new AcpExchange();
agent.respond(await agent.read(), {
  protocolVersion: version,
  agentCapabilities: { loadSession: true },
  agentInfo: { name: "fixture-agent", version: "1.2.3" },
});
await agent.drain();
agent.close();
