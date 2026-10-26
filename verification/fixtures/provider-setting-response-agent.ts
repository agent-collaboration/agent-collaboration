import assert from "node:assert/strict";
import { closeSync } from "node:fs";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { releasePipeBarrier } from "./fixture-pipe-barrier.ts";
import { legacyModes, modeOption } from "./provider-settings-options.ts";
const mode = requireArgument(2);
const agent = new AcpExchange();
const name =
  mode === "held"
    ? "held-setting-fixture"
    : mode === "silent"
      ? "silent-setting-fixture"
      : mode === "legacy"
        ? "legacy-response-fixture"
        : mode === "closed"
          ? "closed-settings-sink"
          : "setting-response-fixture";
await agent.initialize(name);
await agent.newSession(
  "fixture-session",
  mode === "closed"
    ? {}
    : mode === "legacy"
      ? { modes: legacyModes }
      : { configOptions: [modeOption("auto")] },
);
if (mode !== "closed") {
  const request = await agent.read(
    mode === "legacy" ? "session/set_mode" : "session/set_config_option",
  );
  if (["malformed", "disconnect", "rejected"].includes(mode)) {
    assert.equal(field(request, "params", "configId"), "mode");
    assert.equal(field(request, "params", "value"), "ask");
  }
  if (mode === "held") releasePipeBarrier(requireArgument(3), requireArgument(4));
  if (mode === "malformed" || mode === "held")
    agent.respond(request, { configOptions: "bad-shape" });
  else if (mode === "disconnect") closeSync(1);
  else if (mode === "legacy") agent.respond(request, "bad-shape");
  else if (mode === "rejected")
    agent.send({
      jsonrpc: "2.0",
      id: request.id,
      error: { code: -32602, message: "private detail" },
    });
  if (mode === "held" || mode === "rejected") agent.endTurn(await agent.read("session/prompt"));
}
await agent.drain();
agent.close();
