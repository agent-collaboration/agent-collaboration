import assert from "node:assert/strict";
import { field } from "../exact-json.ts";
import { AcpExchange } from "./acp-exchange.ts";

export async function runFixture(): Promise<void> {
  const agent = new AcpExchange();
  await agent.initialize("human-approver-fixture");
  let pending: Record<string, unknown> | undefined;
  let sessions = 0;
  for (;;) {
    const request = await agent.readRequest();
    if (request === undefined) break;
    if (request.method === "session/new") {
      sessions += 1;
      agent.respond(request, { sessionId: `human-approval-session-${sessions}` });
    } else if (request.method === "session/prompt") {
      pending = request;
      agent.send({
        jsonrpc: "2.0",
        id: 91,
        method: "session/request_permission",
        params: {
          sessionId: field(request, "params", "sessionId"),
          toolCall: { toolCallId: "permission-tool" },
          options: [{ optionId: "allow-once", name: "Allow once", kind: "allow_once" }],
        },
      });
    } else {
      assert.equal(request.id, 91n);
      assert.deepEqual(field(request, "result", "outcome"), {
        outcome: "selected",
        optionId: "allow-once",
      });
      assert.ok(pending !== undefined);
      agent.endTurn(pending);
      pending = undefined;
    }
  }
  agent.close();
}
