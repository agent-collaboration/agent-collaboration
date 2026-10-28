import assert from "node:assert/strict";
import { appendFileSync, writeFileSync } from "node:fs";
import { z } from "zod";
import { field } from "../exact-json.ts";
import { requireArgument, wireJson } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { FixtureControlSocket } from "./fixture-control-socket.ts";
import { sendSocketNotice } from "./fixture-socket-notice.ts";
const mode = requireArgument(2);
const agent = new AcpExchange();
const names: Readonly<Record<string, string>> = {
  refusal: "load-fixture",
  new: "no-load-fixture",
  idle: "no-load-fixture",
  close: "close-load-fixture",
  saved: "saved-session-fixture",
  exit: "exited-provider-fixture",
};
const name = names[mode];
if (name === undefined) throw new Error("unknown loaded-route fixture");
agent.respond(
  await agent.read(
    mode === "idle" || mode === "saved" || mode === "close" ? "initialize" : undefined,
  ),
  {
    protocolVersion: 1,
    agentCapabilities:
      mode === "new" || mode === "idle"
        ? {}
        : mode === "close"
          ? { loadSession: true, sessionCapabilities: { close: {} } }
          : { loadSession: true },
    agentInfo: { name, version: "1" },
  },
);
if (mode === "exit") {
  const release = new FixtureControlSocket(
    {
      host: requireArgument(3),
      port: z.coerce.number().int().positive().parse(requireArgument(4)),
    },
    2000,
  );
  try {
    await release.connected();
    await release.expect("X");
  } finally {
    release.close();
  }
  writeFileSync(requireArgument(5), "provider exited");
} else if (mode === "idle") await agent.drain();
else {
  if (mode === "close") await agent.newSession();
  for (;;) {
    const request = await agent.readRequest();
    if (request === undefined) break;
    if (mode === "new") {
      assert.equal(request.method, "session/new");
      agent.respond(request, { sessionId: "fixture-session" });
    } else if (mode === "refusal") {
      assert.equal(request.method, "session/load");
      assert.equal(field(request, "params", "sessionId"), "fixture-session");
      appendFileSync(requireArgument(3), "session/load\n");
      agent.send({
        jsonrpc: "2.0",
        id: request.id,
        error: {
          code: z.coerce.number().int().parse(requireArgument(4)),
          message: "provider fixture refusal",
          data: { privateText: "must not escape" },
        },
      });
    } else if (mode === "close") {
      assert.ok(request.method === "session/close" || request.method === "session/load");
      appendFileSync(
        requireArgument(request.method === "session/close" ? 3 : 4),
        `${request.method}\\n`,
      );
      agent.respond(request, {});
    } else {
      assert.ok(request.method === "session/load" || request.method === "session/prompt");
      assert.equal(field(request, "params", "sessionId"), "fixture-session");
      appendFileSync(requireArgument(3), `${wireJson(request)}\n`);
      agent.respond(request, request.method === "session/load" ? {} : { stopReason: "end_turn" });
      if (request.method === "session/prompt")
        await sendSocketNotice(requireArgument(4), "observed");
    }
  }
}
agent.close();
