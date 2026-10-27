import { appendFileSync, writeFileSync } from "node:fs";
import { textField } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const scenario = requireArgument(2);
const agent = new AcpExchange();
const saturated = scenario === "saturated" || scenario === "supervisor-saturated";
if (saturated) writeFileSync(requireArgument(4), String(process.pid));
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: { loadSession: true },
  agentInfo: {
    name:
      scenario === "supervisor-saturated"
        ? "saturated-supervisor-fixture"
        : saturated
          ? "saturated-load-fixture"
          : "concurrent-fixture",
    version: "1",
  },
});
if (saturated) {
  for (;;) {
    const request = await agent.readRequest();
    if (request === undefined) break;
    if (request.method !== "session/load") throw new Error("expected session/load");
    appendFileSync(requireArgument(3), `${textField(request, "params", "sessionId")}\n`);
  }
} else {
  agent.respond(await agent.read(), { sessionId: "active-session" });
  const prompt = await agent.read();
  const admitted = await agent.read(requireArgument(3));
  writeFileSync(requireArgument(4), "observed");
  await agent.read("session/cancel");
  agent.endTurn(prompt, "cancelled");
  agent.respond(admitted, admitted.method === "session/new" ? { sessionId: "new-session" } : {});
  await agent.drain();
}
agent.close();
