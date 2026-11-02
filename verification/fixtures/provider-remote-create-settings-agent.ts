import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { selectOption } from "./provider-settings-options.ts";

const receipt = requireArgument(2);
const methods: string[] = [];
const agent = new AcpExchange((request): void => {
  if (typeof request.method !== "string") throw new Error("expected ACP method");
  methods.push(request.method);
  writeFileSync(receipt, JSON.stringify(methods));
});
await agent.initialize("remote-create-settings-fixture", {
  agentCapabilities: { sessionCapabilities: { close: {} } },
});
await agent.newSession("remote-created-session", {
  configOptions: [
    selectOption({ id: "model", current: "a", values: ["a", "b"] }),
    selectOption({
      id: "effort",
      current: "low",
      values: ["low", "high"],
      category: "thought_level",
    }),
  ],
});
for (;;) {
  const request = await agent.readRequest();
  if (request === undefined) break;
  if (request.method === "session/close") agent.respond(request, {});
  else {
    assert.equal(request.method, "session/prompt");
    agent.endTurn(request);
  }
}
agent.close();
