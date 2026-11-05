import assert from "node:assert/strict";
import { appendFileSync, writeFileSync } from "node:fs";
import { z } from "zod";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { selectOption } from "./provider-settings-options.ts";
const mode = requireArgument(2);
const authenticationScenario =
  mode === "initialization-authentication"
    ? z
        .enum(["success", "held", "reject", "malformed", "timeout", "close", "not-requested"])
        .parse(requireArgument(4))
    : undefined;
if (mode === "initialization-authentication")
  writeFileSync(`${requireArgument(3)}.pid`, `${process.pid}`);
const agent = new AcpExchange(
  mode === "initialization-authentication"
    ? (request): void => {
        const method = z.string().parse(request.method);
        appendFileSync(requireArgument(3), `${method}\n`);
      }
    : undefined,
);
const names: Readonly<Record<string, string>> = {
  cancel: "supervisor-fixture",
  load: "load-fixture",
  lifecycle: "lifecycle-fixture",
  permission: "permission-fixture",
  retirement: "permission-retirement-fixture",
  authentication: "auth-fixture",
  "initialization-authentication": "initialization-auth-fixture",
  "prompt-authentication": "prompt-auth-fixture",
  rejection: "prompt-rejection-fixture",
  loss: "loss-fixture",
};
const name = names[mode];
if (name === undefined) throw new Error("unknown supervisor integration scenario");
const capabilities =
  mode === "cancel" || mode === "load"
    ? { loadSession: true }
    : mode === "lifecycle"
      ? { sessionCapabilities: { resume: {}, close: {} } }
      : {};
agent.respond(await agent.read(mode === "lifecycle" ? "initialize" : undefined), {
  protocolVersion: 1,
  agentCapabilities: capabilities,
  agentInfo: { name, version: "1" },
  ...(mode === "initialization-authentication"
    ? { authMethods: [{ id: "cursor_login", name: "Cursor login" }] }
    : {}),
});
if (mode === "loss") await agent.nextLine();
else if (mode === "load") agent.respond(await agent.read("session/load"), {});
else if (mode === "lifecycle") {
  agent.respond(await agent.read("session/resume"), {});
  agent.respond(await agent.read("session/close"), {});
} else if (mode === "initialization-authentication") {
  if (authenticationScenario !== "not-requested") {
    const authentication = await agent.read("authenticate");
    assert.deepEqual(authentication.params, { methodId: "cursor_login" });
    if (authenticationScenario === "held" || authenticationScenario === "timeout")
      await agent.socketSignal(
        requireArgument(5),
        (): Error => new Error("request arrived before authentication reply was released"),
      );
    if (authenticationScenario === "reject")
      agent.send({
        jsonrpc: "2.0",
        id: authentication.id,
        error: { code: -32000, message: "authentication required" },
      });
    else if (authenticationScenario === "malformed") agent.respond(authentication, false);
    else if (authenticationScenario === "close") agent.close();
    else if (authenticationScenario !== "timeout") {
      agent.respond(authentication, {});
      agent.respond(await agent.read("cursor/list_available_models"), { models: [] });
    }
  }
  if (["success", "held", "not-requested"].includes(authenticationScenario ?? "")) {
    const options = (current: string): Record<string, unknown>[] => [
      selectOption({ id: "effort", current, values: ["low", "high"], category: "thought_level" }),
    ];
    await agent.newSession("authenticated-session-1", { configOptions: options("low") });
    await agent.newSession("authenticated-session-2", { configOptions: options("low") });
    const setting = await agent.read("session/set_config_option");
    assert.equal(field(setting, "params", "configId"), "effort");
    assert.equal(field(setting, "params", "value"), "high");
    agent.respond(setting, { configOptions: options("high") });
  }
} else if (mode === "authentication") {
  const first = await agent.read();
  agent.send({
    jsonrpc: "2.0",
    id: first.id,
    error: { code: -32000, message: "authentication required" },
  });
  agent.respond(await agent.read(), { sessionId: "authenticated-session" });
} else {
  const created = await agent.read(
    mode === "cancel" || mode === "permission" ? "session/new" : undefined,
  );
  if (mode === "cancel") appendFileSync(requireArgument(3), "new\n");
  agent.respond(created, {
    sessionId:
      mode === "prompt-authentication"
        ? "authenticated-session"
        : mode === "rejection"
          ? "rejection-session"
          : "fixture-session",
  });
  const prompt = await agent.read(
    mode === "cancel" || mode === "permission" || mode === "prompt-authentication"
      ? "session/prompt"
      : undefined,
  );
  if (mode === "cancel") {
    appendFileSync(requireArgument(3), "prompt\n");
    await agent.read("session/cancel");
    appendFileSync(requireArgument(3), "cancel\n");
    agent.endTurn(prompt, "cancelled");
  } else if (mode === "permission" || mode === "retirement") {
    const id = mode === "retirement" ? 92 : 91;
    const options = [
      { optionId: "allow-exact-once", name: "Allow once", kind: "allow_once" },
      ...(mode === "permission"
        ? [{ optionId: "deny-exact-once", name: "Deny once", kind: "reject_once" }]
        : []),
    ];
    agent.send({
      jsonrpc: "2.0",
      id,
      method: "session/request_permission",
      params: {
        sessionId: "fixture-session",
        toolCall: { toolCallId: "permission-tool" },
        options,
      },
    });
    const response = await agent.read();
    assert.equal(response.id, BigInt(id));
    assert.equal(
      field(response, "result", "outcome", "outcome"),
      mode === "permission" ? "selected" : "cancelled",
    );
    if (mode === "permission")
      assert.equal(field(response, "result", "outcome", "optionId"), "allow-exact-once");
    agent.endTurn(prompt);
  } else if (mode === "prompt-authentication") {
    agent.send({
      jsonrpc: "2.0",
      id: prompt.id,
      error: { code: -32000, message: "authentication required" },
    });
    const second = await agent.read("session/prompt");
    agent.update(
      {
        sessionUpdate: "agent_message_chunk",
        content: { type: "text", text: "final text retained" },
      },
      "authenticated-session",
    );
    agent.endTurn(second);
  } else if (mode === "rejection")
    agent.send({
      jsonrpc: "2.0",
      id: prompt.id,
      error: { code: -32603, message: "provider conversation is busy" },
    });
}
if (mode !== "loss") await agent.drain();
agent.close();
