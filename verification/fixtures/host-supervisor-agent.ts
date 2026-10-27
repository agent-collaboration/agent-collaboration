import assert from "node:assert/strict";
import { arrayField, textField } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { sendSocketNotice } from "./fixture-socket-notice.ts";
const mode = requireArgument(2);
const agent = new AcpExchange();
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: {},
  agentInfo: {
    name:
      mode === "create" ? "race-fixture" : mode === "pending" ? "dispatch-fixture" : "fifo-fixture",
    version: "1",
  },
});
agent.respond(await agent.read(mode === "create" ? undefined : "session/new"), {
  sessionId: "fixture-session",
});
if (mode === "pending") await agent.read("session/prompt");
else if (mode === "ordered")
  for (const expected of ["first", "second"]) {
    const request = await agent.read("session/prompt");
    const text = textField(arrayField(request, "params", "prompt")[0], "text");
    assert.ok(text.startsWith(`✉️ sender · "${expected}" · router://`));
    const suffix = text.slice(text.indexOf("router://") + "router://".length);
    const separator = suffix.indexOf("/push/");
    assert.ok(separator > 0 && suffix.slice(separator + "/push/".length).length > 0);
    await sendSocketNotice(requireArgument(3), text);
    agent.endTurn(request);
  }
await agent.drain();
agent.close();
