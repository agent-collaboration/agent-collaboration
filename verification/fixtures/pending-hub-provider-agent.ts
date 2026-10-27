import assert from "node:assert/strict";
import { once } from "node:events";
import { createServer } from "node:net";
import { z } from "zod";
import { field, textField } from "../exact-json.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { wireJson } from "./acp-stdio.ts";

export async function runFixture(_configuration: unknown): Promise<void> {
  let release: () => void = (): void => {};
  const cancelledPromptReleased = new Promise<void>((resolve): void => {
    release = resolve;
  });
  const control = createServer((peer): void => {
    peer.once("data", (bytes: Buffer): void => {
      const command = bytes.subarray(0, 1).toString();
      peer.destroy();
      if (command === "x") process.exit(0);
      if (command === "c") release();
    });
  });
  control.listen({ path: z.string().parse(process.argv[2]), backlog: 1 });
  await once(control, "listening");
  const agent = new AcpExchange();
  await agent.initialize("pending-hub-fixture");
  let nextSession = 0;
  const pending = new Map<string, unknown>();
  const responded = new Set<string>();
  function finishCancelled(session: string): void {
    assert.ok(pending.has(session));
    const id = pending.get(session);
    pending.delete(session);
    agent.endTurn({ id }, "cancelled");
    responded.add(session);
  }
  for (;;) {
    const request = await agent.readRequest();
    if (request === undefined) break;
    if (request.method === "session/new") {
      nextSession++;
      agent.respond(request, { sessionId: `fixture-${nextSession}` });
    } else if (request.method === "session/prompt") {
      const session = textField(request, "params", "sessionId");
      if (session === "fixture-1") {
        pending.set(session, request.id);
        agent.send({
          jsonrpc: "2.0",
          id: 91,
          method: "session/request_permission",
          params: {
            sessionId: session,
            toolCall: { toolCallId: "permission-tool" },
            options: [{ optionId: "allow-once", name: "Allow once", kind: "allow_once" }],
          },
        });
        await cancelledPromptReleased;
        finishCancelled(session);
      } else agent.endTurn(request);
    } else if (request.method === "session/cancel") {
      const session = textField(request, "params", "sessionId");
      if (!responded.has(session)) finishCancelled(session);
    } else if (request.id === 91n) {
      assert.equal(field(request, "result", "outcome", "outcome"), "cancelled");
      if (!responded.has("fixture-1")) finishCancelled("fixture-1");
    } else assert.fail(wireJson(request));
  }
  agent.close();
  control.unref();
}
