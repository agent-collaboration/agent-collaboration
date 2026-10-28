import assert from "node:assert/strict";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
const scenario = requireArgument(2);
const agent = new AcpExchange();
const name =
  scenario === "injected"
    ? "input-fixture"
    : scenario === "started"
      ? "started-steer-fixture"
      : "prompt-required-fixture";
await agent.initialize(name, { _meta: { steering: { supported: true } } });
await agent.newSession();
const prompt = scenario === "injected" ? await agent.read("session/prompt") : undefined;
const steer = await agent.read("_session/steering");
if (scenario === "required")
  assert.equal(field(steer, "params", "_meta", "steering", "idleBehavior"), "promptRequired");
agent.respond(steer, {
  outcome:
    scenario === "injected"
      ? "injected"
      : scenario === "started"
        ? "startedNewTurn"
        : "promptRequired",
});
if (prompt !== undefined) agent.endTurn(prompt);
else if (scenario === "required") agent.endTurn(await agent.read("session/prompt"));
await agent.drain();
agent.close();
