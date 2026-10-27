import assert from "node:assert/strict";
import { once } from "node:events";
import { createConnection } from "node:net";
import { z } from "zod";
import { field } from "../exact-json.ts";
import { AcpExchange } from "./acp-exchange.ts";

export async function runFixture(configuration: unknown): Promise<void> {
  const { mode } = z
    .object({ mode: z.enum(["composition", "acp", "tui", "startup", "failure", "exit"]) })
    .parse(configuration);
  if (mode === "failure") {
    process.exitCode = 4;
    return;
  }
  const agent = new AcpExchange();
  const initialized = await agent.read();
  if (mode === "exit") {
    const barrier = createConnection(z.string().parse(process.argv[2]));
    try {
      await once(barrier, "connect");
      const pid = Buffer.alloc(4);
      pid.writeUInt32BE(process.pid);
      barrier.write(pid);
      await new Promise<void>((resolve, reject): void => {
        barrier.once("data", (): void => {
          resolve();
        });
        barrier.once("end", resolve);
        barrier.once("error", reject);
      });
    } finally {
      barrier.destroy();
    }
    agent.respond(initialized, {
      protocolVersion: 1,
      agentCapabilities: {},
      agentInfo: { name: "exit-fixture", version: "1" },
    });
    agent.close();
    return;
  }
  const name =
    mode === "composition"
      ? "composition-fixture"
      : mode === "acp"
        ? "acp-fixture"
        : mode === "tui"
          ? "tui-fixture"
          : "startup-fixture";
  agent.respond(initialized, {
    protocolVersion: 1,
    agentCapabilities: mode === "composition" || mode === "startup" ? { loadSession: true } : {},
    agentInfo: { name, version: "1" },
    ...(mode === "composition" ? { _meta: { steering: { supported: true } } } : {}),
  });
  if (mode !== "startup") {
    const sessionId =
      mode === "composition"
        ? "provider-session"
        : mode === "acp"
          ? "acp-provider-session"
          : "tui-provider-session";
    await agent.newSession(sessionId);
    if (mode === "composition") {
      agent.endTurn(await agent.read("session/prompt"));
      agent.respond(await agent.read("_session/steering"), { outcome: "promptRequired" });
      agent.endTurn(await agent.read("session/prompt"));
    } else if (mode === "tui")
      for (const [index, text] of [
        "hello from TUI",
        "queued by another agent",
        "prompt by another agent",
      ].entries()) {
        const prompt = await agent.read("session/prompt");
        assert.deepEqual(field(prompt, "params", "prompt"), [{ type: "text", text }]);
        if (index === 0)
          agent.update(
            {
              sessionUpdate: "agent_message_chunk",
              content: { type: "text", text: "fixture reply" },
            },
            sessionId,
          );
        agent.endTurn(prompt);
      }
  }
  await agent.drain();
  agent.close();
}
