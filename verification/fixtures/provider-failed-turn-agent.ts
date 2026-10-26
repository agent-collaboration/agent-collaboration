import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { arrayField, field, textField } from "../exact-json.ts";
import { requireArgument, wireJson } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { FixtureControlSocket, withinFixtureDeadline } from "./fixture-control-socket.ts";
const scenario = requireArgument(2);
const controlPath = requireArgument(3);
const receipt = requireArgument(4);
const texts: string[] = [];
const agent = new AcpExchange((request): void => {
  if (request.method === "session/prompt") {
    texts.push(textField(arrayField(request, "params", "prompt")[0], "text"));
    writeFileSync(receipt, wireJson({ promptCount: texts.length, promptTexts: texts }));
  }
});
async function read(method?: string): Promise<Record<string, unknown>> {
  return withinFixtureDeadline(agent.read(method), 5000);
}
const control = new FixtureControlSocket(controlPath);
try {
  await control.connected();
  const sessionId =
    scenario === "cancel"
      ? "fixture-session"
      : scenario === "tool-plan"
        ? "failed-tool-plan-session"
        : "failed-stream-session";
  const initialized = await read(scenario === "cancel" ? undefined : "initialize");
  agent.respond(initialized, {
    protocolVersion: 1,
    agentCapabilities: {},
    agentInfo: {
      name:
        scenario === "cancel"
          ? "cancel-order-fixture"
          : scenario === "tool-plan"
            ? "failed-tool-plan-fixture"
            : "failed-stream-order-fixture",
      version: "1",
    },
  });
  agent.respond(await read(scenario === "cancel" ? undefined : "session/new"), { sessionId });
  const first = await read("session/prompt");
  if (scenario !== "cancel")
    assert.deepEqual(field(first, "params", "prompt"), [{ type: "text", text: "FIRST_REQUEST" }]);
  agent.update(
    {
      sessionUpdate: "agent_message_chunk",
      content: {
        type: "text",
        text: scenario === "cancel" ? "x".repeat(1048577) : "FIRST_PARTIAL",
      },
    },
    sessionId,
  );
  if (scenario === "cancel") {
    await read("session/cancel");
    agent.endTurn(first, "cancelled");
    await control.expect("g");
    agent.update({
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "LATE_OUTPUT" },
    });
    agent.send({ jsonrpc: "2.0", id: 99, method: "probe/unknown", params: {} });
    const barrier = await read();
    assert.equal(barrier.id, 99n);
    assert.ok(Object.hasOwn(barrier, "error"));
    control.send("r");
  } else {
    if (scenario === "tool-plan") {
      agent.update(
        {
          sessionUpdate: "tool_call",
          toolCallId: "failed-tool",
          title: "FIRST_TOOL",
          kind: "execute",
          status: "pending",
        },
        sessionId,
      );
      agent.update(
        {
          sessionUpdate: "plan",
          entries: [{ content: "FIRST_PLAN", priority: "medium", status: "pending" }],
        },
        sessionId,
      );
    }
    await control.expect("e");
    agent.send({
      jsonrpc: "2.0",
      id: first.id,
      error: { code: -32603, message: "first fixture prompt failed" },
    });
    if (scenario === "tool-plan") {
      await control.expect("t");
      agent.update(
        { sessionUpdate: "tool_call_update", toolCallId: "failed-tool", status: "completed" },
        sessionId,
      );
    }
    await control.expect("s");
  }
  const second = await read("session/prompt");
  if (scenario !== "cancel")
    assert.deepEqual(field(second, "params", "prompt"), [{ type: "text", text: "SECOND_REQUEST" }]);
  if (scenario === "tool-plan")
    agent.update(
      {
        sessionUpdate: "plan",
        entries: [{ content: "SECOND_PLAN", priority: "medium", status: "pending" }],
      },
      sessionId,
    );
  agent.update(
    {
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: scenario === "cancel" ? "CURRENT_OUTPUT" : "SECOND_ONLY" },
    },
    sessionId,
  );
  agent.endTurn(second);
  if (scenario !== "cancel") {
    control.send("d");
    await control.expect("q");
    control.close();
  }
  await withinFixtureDeadline(agent.drain(), 15_000);
} finally {
  control.close();
  agent.close();
}
