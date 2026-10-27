import assert from "node:assert/strict";
import { appendFileSync } from "node:fs";
import { AcpExchange } from "./acp-exchange.ts";
import { requireArgument, wireJson } from "./acp-stdio.ts";

export async function runFixture(): Promise<void> {
  const log = requireArgument(2);
  const agent = new AcpExchange();
  await agent.initialize("source-policy-fixture", {
    agentCapabilities: { loadSession: true, sessionCapabilities: { resume: {} } },
  });
  for (;;) {
    const request = await agent.readRequest();
    if (request === undefined) break;
    assert.ok(["session/new", "session/load", "session/resume"].includes(String(request.method)));
    appendFileSync(log, `${wireJson(request)}\n`);
    agent.respond(
      request,
      request.method === "session/new" ? { sessionId: "created-by-source" } : {},
    );
  }
  agent.close();
}
