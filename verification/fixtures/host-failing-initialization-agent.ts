import { closeSync, existsSync, writeFileSync } from "node:fs";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const scenario = requireArgument(2);
writeFileSync(requireArgument(3), String(process.pid));
const agent = new AcpExchange();
if (scenario === "eof") {
  agent.respond(await agent.read(), {
    protocolVersion: 1,
    agentCapabilities: {},
    agentInfo: { name: "eof-fixture", version: "1" },
  });
  while (!existsSync(requireArgument(4))) await setTimeout(10);
  closeSync(1);
  await agent.drain();
} else {
  await agent.nextLine();
  if (scenario === "held") await setTimeout(60_000);
  else {
    const limit = z.coerce.number().int().positive().parse(requireArgument(4));
    process.stdout.write(`${"x".repeat(limit + 1)}\n`);
    await agent.drain();
  }
}
agent.close();
