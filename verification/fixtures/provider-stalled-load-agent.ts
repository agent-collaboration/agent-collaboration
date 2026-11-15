import { z } from "zod";
import { AcpExchange } from "./acp-exchange.ts";
import { sendSocketNotice } from "./fixture-socket-notice.ts";

/// A provider that accepts `session/load` and never answers it, so the Router's
/// load wait has no reply to end it. The test learns of the request through the
/// notice socket.
export async function runFixture(configuration: unknown): Promise<void> {
  const { loadNoticeSocket } = z
    .strictObject({ loadNoticeSocket: z.string().min(1) })
    .parse(configuration);
  const agent = new AcpExchange();
  agent.respond(await agent.read("initialize"), {
    protocolVersion: 1,
    agentCapabilities: { loadSession: true },
    agentInfo: { name: "stalled-load-fixture", version: "1" },
    authMethods: [{ id: "cursor_login", name: "Cursor login" }],
  });
  for (;;) {
    const request = await agent.readRequest();
    if (request === undefined) break;
    if (request.method === "authenticate") {
      agent.respond(request, {});
      continue;
    }
    if (request.method === "session/load") {
      await sendSocketNotice(loadNoticeSocket, "load\n");
      continue;
    }
    agent.send({
      jsonrpc: "2.0",
      id: request.id,
      error: {
        code: -32601,
        message: `stalled-load fixture does not serve ${String(request.method)}`,
      },
    });
  }
  agent.close();
}
