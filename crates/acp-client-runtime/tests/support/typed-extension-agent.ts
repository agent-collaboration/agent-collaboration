import { AcpExchange } from "../../../../verification/fixtures/acp-exchange.ts";
import { requireArgument } from "../../../../verification/fixtures/acp-stdio.ts";

// Scripted agent for the typed extension messages: `_session/steering` and
// `cursor/list_available_models`. The receipt is the shared transport's complete wire capture:
// every line the agent received and every line it sent, in order, from `initialize` on.
const scenario = requireArgument(2);
const receipt = requireArgument(3);
const agent = new AcpExchange(undefined, { wireCapture: receipt });

await agent.initialize("typed-extension-fixture", {
  agentCapabilities: { promptCapabilities: { image: true } },
  _meta: { steering: { supported: true } },
});

if (scenario.startsWith("models-")) {
  const request = await agent.read("cursor/list_available_models");
  if (scenario === "models-ok")
    agent.respond(request, {
      models: [
        {
          value: "model-a",
          name: "Model A",
          unknownModelField: true,
          configOptions: [
            {
              id: "effort",
              name: "Effort",
              category: "thought_level",
              type: "select",
              currentValue: "low",
              options: [
                { value: "low", name: "Low" },
                { value: "high", name: "High" },
              ],
            },
          ],
        },
        { value: "model-b", name: "Model B", configOptions: [] },
      ],
      unknownTopLevelField: "ignored",
    });
  else
    agent.send({
      jsonrpc: "2.0",
      id: request.id,
      error: { code: -32601, message: "scripted discovery failure" },
    });
  await agent.newSession();
} else if (scenario === "steer-idle-prompt") {
  await agent.newSession();
  agent.endTurn(await agent.read("session/prompt"));
} else {
  await agent.newSession();
  const prompt = await agent.read("session/prompt");
  const steer = await agent.read("_session/steering");
  // promptRequired carries Claude's reason, as the Claude extras pin defines it. `failed` is not a
  // Claude outcome and `sideways` is outside every contract: the test injects both.
  const result = {
    "steer-injected": { outcome: "injected" },
    "steer-started-new-turn": { outcome: "startedNewTurn" },
    "steer-prompt-required": { outcome: "promptRequired", reason: "noRunningTurn" },
    "steer-failed": { outcome: "failed" },
    "steer-invalid": { outcome: "sideways" },
  }[scenario];
  if (result === undefined) throw new Error("unknown typed extension scenario");
  agent.respond(steer, result);
  // The turn stays open until the test cancels it, so nothing ends it between the reply and the tap.
  if (scenario === "steer-prompt-required") {
    agent.endTurn(prompt);
    agent.endTurn(await agent.read("session/prompt"));
  } else {
    await agent.read("session/cancel");
    agent.endTurn(prompt, "cancelled");
  }
}
await agent.drain();
agent.close();
