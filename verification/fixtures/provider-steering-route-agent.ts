import assert from "node:assert/strict";
import { arrayField, field, textField } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { FixtureControlSocket } from "./fixture-control-socket.ts";
import { sendSocketNotice } from "./fixture-socket-notice.ts";
const mode = requireArgument(2);
const agent = new AcpExchange();
const advertised = mode !== "none" && mode !== "serial";
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: { loadSession: true },
  agentInfo: {
    name:
      mode === "none"
        ? "no-steer-fixture"
        : mode === "serial"
          ? "no-steer-prompt"
          : "claude-fixture",
    version: "1",
  },
  ...(advertised ? { _meta: { steering: { supported: true } } } : {}),
});
agent.respond(await agent.read(mode === "serial" ? undefined : "session/new"), {
  sessionId: "fixture-session",
});
function requireRoutedText(request: unknown): void {
  const text = textField(arrayField(request, "params", "prompt")[0], "text");
  assert.ok(text.includes('"follow-up"') && text.includes("router://"));
}
if (mode === "active") {
  const prompt = await agent.read("session/prompt");
  const event = new FixtureControlSocket(requireArgument(3));
  try {
    await event.connected();
    event.send("active");
    for (let index = 0; index < 3; index++) {
      const steer = await agent.read("_session/steering");
      if (index < 2) requireRoutedText(steer);
      else
        assert.deepEqual(field(steer, "params", "prompt"), [{ type: "text", text: "follow-up" }]);
      agent.respond(steer, { outcome: "injected" });
    }
    await event.waitForByte();
  } finally {
    event.close();
  }
  agent.endTurn(prompt);
} else if (mode === "serial") {
  const first = await agent.read("session/prompt");
  requireRoutedText(first);
  const event = new FixtureControlSocket(requireArgument(3));
  try {
    await event.connected();
    event.send("first");
    await event.waitForByte();
  } finally {
    event.close();
  }
  agent.endTurn(first);
  const second = await agent.read("session/prompt");
  requireRoutedText(second);
  await sendSocketNotice(requireArgument(3), "second");
  agent.endTurn(second);
} else if (mode !== "none") {
  const steer = await agent.read("_session/steering");
  if (mode === "drop") {
    agent.close();
  } else {
    agent.respond(
      steer,
      mode === "started"
        ? { outcome: "startedNewTurn" }
        : mode === "failed"
          ? { outcome: "failed", reason: "private provider detail" }
          : { outcome: "promptRequired" },
    );
    if (mode === "auto") agent.endTurn(await agent.read("session/prompt"));
  }
}
if (mode !== "drop") await agent.drain();
agent.close();
