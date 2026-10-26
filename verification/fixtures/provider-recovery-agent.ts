import assert from "node:assert/strict";
import { z } from "zod";
import { appendFileSync, writeFileSync } from "node:fs";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const mode = requireArgument(2);
const log = requireArgument(3);
const agent = new AcpExchange();
if (mode === "held") writeFileSync(requireArgument(4), String(process.pid));
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: { loadSession: true },
  agentInfo: { name: mode === "held" ? "recovery-fixture" : "reopened-fixture", version: "1" },
});
if (mode === "held") {
  const request = await agent.read(requireArgument(5));
  appendFileSync(log, `${request.method}\n`);
  await agent.drain();
} else
  for (;;) {
    const request = await agent.readRequest();
    if (request === undefined) break;
    appendFileSync(
      log,
      `${Object.hasOwn(request, "method") ? z.string().parse(request.method) : "unknown"}\n`,
    );
    assert.ok(Object.hasOwn(request, "method"));
    agent.respond(
      request,
      request.method === "session/new" ? { sessionId: "fresh-provider-session" } : {},
    );
  }
agent.close();
