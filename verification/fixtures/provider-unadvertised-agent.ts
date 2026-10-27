import { writeFileSync } from "node:fs";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const marker = requireArgument(2);
const agent = new AcpExchange();
await agent.initialize("no-lifecycle-fixture");
if ((await agent.nextLine()) !== undefined) writeFileSync(marker, "unexpected lifecycle request");
agent.close();
