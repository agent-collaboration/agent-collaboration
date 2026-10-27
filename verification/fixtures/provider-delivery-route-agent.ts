import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { FixtureControlSocket } from "./fixture-control-socket.ts";
import { sendSocketNotice } from "./fixture-socket-notice.ts";
const mode = requireArgument(2);
const agent = new AcpExchange();
agent.respond(await agent.read(), {
  protocolVersion: 1,
  agentCapabilities: mode === "queue" ? { loadSession: true } : {},
  agentInfo: { name: mode === "queue" ? "queue-fixture" : "cursor-fixture", version: "1" },
  ...(mode === "queue" ? { _meta: { steering: { supported: true } } } : {}),
});
await agent.newSession();
const first = await agent.read("session/prompt");
if (mode === "queue") {
  await sendSocketNotice(requireArgument(3), "queued-prompt");
  agent.endTurn(first);
} else {
  const held = new FixtureControlSocket(requireArgument(3));
  try {
    await held.connected();
    held.send("first");
    await held.waitForByte();
  } finally {
    held.close();
  }
  agent.endTurn(first);
  const second = await agent.read("session/prompt");
  agent.receipt(requireArgument(4), second);
  await sendSocketNotice(requireArgument(3), "second");
  agent.endTurn(second);
}
await agent.drain();
agent.close();
