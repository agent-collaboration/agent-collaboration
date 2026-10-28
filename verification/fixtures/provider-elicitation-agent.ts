import assert from "node:assert/strict";
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";

const mode = requireArgument(2);
const receipt = requireArgument(3);
const agent = new AcpExchange();
const initialized = await agent.read("initialize");
assert.deepEqual(field(initialized, "params", "clientCapabilities", "elicitation"), { form: {} });
agent.respond(initialized, {
  protocolVersion: 1,
  agentCapabilities: {},
  agentInfo: { name: "elicitation-fixture", version: "1" },
});
await agent.newSession();
const prompt = await agent.read("session/prompt");
const params =
  mode === "unsupported"
    ? {
        mode: "url",
        sessionId: "fixture-session",
        message: "Open a link",
        url: "https://example.invalid",
      }
    : {
        mode: "form",
        sessionId: "fixture-session",
        message: "Choose settings",
        requestedSchema: {
          type: "object",
          properties: {
            count: { type: "number", title: "Count" },
            active: { type: "boolean", title: "Active" },
            color: {
              type: "string",
              title: "Color",
              oneOf: [
                { const: "blue", title: "Blue" },
                { const: "red", title: "Red" },
              ],
            },
            tags: {
              type: "array",
              title: "Tags",
              items: {
                type: "string",
                anyOf: [
                  { const: "a", title: "Alpha" },
                  { const: "b", title: "Beta" },
                ],
              },
            },
          },
          required: ["count", "color"],
        },
      };
agent.send({ jsonrpc: "2.0", id: "elicit-one", method: "elicitation/create", params });
await agent.responseReceipt(receipt, "elicit-one");
agent.endTurn(prompt);
await agent.drain();
agent.close();
