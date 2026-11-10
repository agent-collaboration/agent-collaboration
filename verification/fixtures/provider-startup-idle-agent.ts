/** Startup-only ACP counterpart: initialize, then require EOF without session/model input. */
import assert from "node:assert/strict";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";

const receipt = requireArgument(2);
const agent = new AcpExchange();
await agent.initialize("worker-startup-fixture");
agent.receipt(receipt, { initialized: true, pid: process.pid, additionalRequests: 0 });
const unexpected = await agent.readRequest();
if (unexpected !== undefined)
  agent.receipt(receipt, { initialized: true, pid: process.pid, additionalRequests: 1 });
assert.equal(
  unexpected === undefined,
  true,
  "startup observation permits no session or model requests",
);
agent.receipt(receipt, {
  initialized: true,
  pid: process.pid,
  additionalRequests: 0,
  closed: true,
});
agent.close();
