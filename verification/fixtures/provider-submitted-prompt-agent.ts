import { appendFileSync } from "node:fs";
import { arrayField, textField } from "../exact-json.ts";
import { requireArgument, wireJson } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";

const mode = requireArgument(2);
const receipt = requireArgument(3);
const agent = new AcpExchange();
await agent.initialize("submitted-prompt-fixture");
await agent.newSession();
let promptCount = 0;
try {
  for (;;) {
    const request = await agent.readRequest();
    if (request === undefined) break;
    if (request.method !== "session/prompt") continue;
    promptCount += 1;
    const blocks = arrayField(request, "params", "prompt");
    appendFileSync(receipt, `${wireJson(blocks)}\n`);
    if (mode === "echo" || mode === "different") {
      const text = mode === "echo" ? textField(blocks[0], "text") : "Native different task";
      agent.update({
        sessionUpdate: "user_message_chunk",
        messageId: `native-${promptCount}`,
        content: { type: "text", text },
      });
      agent.update({
        sessionUpdate: "user_message_chunk",
        messageId: `native-${promptCount}`,
        content: { type: "text", text: " + native tail" },
      });
    }
    if (mode === "reject") {
      agent.send({
        jsonrpc: "2.0",
        id: request.id,
        error: { code: -32603, message: "fixture prompt rejection" },
      });
    } else {
      agent.update({
        sessionUpdate: "agent_message_chunk",
        messageId: `reply-${promptCount}`,
        content: { type: "text", text: "Actual fixture reply" },
      });
      agent.endTurn(request);
    }
  }
} finally {
  agent.close();
}
