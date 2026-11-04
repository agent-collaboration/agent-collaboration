import assert from "node:assert/strict";
import { appendFileSync } from "node:fs";
import { field, textField } from "../exact-json.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { requireArgument, wireJson } from "./acp-stdio.ts";

export async function runFixture(): Promise<void> {
  const sessionId = requireArgument(2);
  const loadMarker = requireArgument(3);
  const promptLog = requireArgument(4);
  const agent = new AcpExchange();
  agent.respond(await agent.read(), {
    protocolVersion: 1,
    agentCapabilities: { loadSession: true },
    agentInfo: { name: "delivery-fixture", version: "1" },
    _meta: { steering: { supported: true } },
  });
  for (;;) {
    const request = await agent.readRequest();
    if (request === undefined) break;
    if (request.method === "cursor/list_available_models") {
      agent.send({
        jsonrpc: "2.0",
        id: request.id,
        error: { code: -32601, message: "Model discovery is unsupported by this routing fixture" },
      });
      continue;
    }
    let result: unknown;
    if (request.method === "session/new") result = { sessionId };
    else if (request.method === "session/load") {
      appendFileSync(loadMarker, "load\n");
      result = {};
    } else if (request.method === "_session/steering") result = { outcome: "promptRequired" };
    else if (request.method === "session/prompt") {
      appendFileSync(promptLog, `${wireJson(request)}\n`);
      const prompt = field(request, "params", "prompt");
      assert.ok(Array.isArray(prompt));
      if (textField(prompt[0], "text").includes("request permission")) {
        agent.send({
          jsonrpc: "2.0",
          id: 91,
          method: "session/request_permission",
          params: {
            sessionId,
            toolCall: { toolCallId: "permission-tool" },
            options: [
              { optionId: "allow-once", name: "Allow once", kind: "allow_once" },
              { optionId: "deny-once", name: "Deny once", kind: "reject_once" },
            ],
          },
        });
        const decision = await agent.read();
        assert.equal(decision.id, 91n);
        assert.equal(field(decision, "result", "outcome", "outcome"), "selected");
        assert.equal(field(decision, "result", "outcome", "optionId"), "allow-once");
        appendFileSync(promptLog, `${wireJson({ fixturePermissionDecision: decision })}\n`);
      }
      result = { stopReason: "end_turn" };
    } else throw new Error(String(request.method));
    agent.respond(request, result);
    appendFileSync(promptLog, `${wireJson({ fixturePromptCompleted: request.id })}\n`);
  }
  agent.close();
}
