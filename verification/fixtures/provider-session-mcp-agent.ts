import { z } from "zod";
import { AcpExchange } from "./acp-exchange.ts";
import { requireArgument } from "./acp-stdio.ts";

const mode = z.enum(["http", "no-http"]).parse(requireArgument(2));
const receiptPath = requireArgument(3);
const agent = new AcpExchange();
const requests: Readonly<Record<string, unknown>>[] = [];
agent.receipt(receiptPath, requests);
await agent.initialize("session-mcp-fixture", {
  agentCapabilities: {
    loadSession: true,
    mcpCapabilities: { http: mode === "http" },
    sessionCapabilities: { resume: {}, close: {} },
  },
});

let createdSessions = 0;
for (;;) {
  const request = await agent.readRequest();
  if (request === undefined) break;
  requests.push(request);
  agent.receipt(receiptPath, requests);
  const method = z
    .enum(["session/new", "session/load", "session/resume", "session/close"])
    .parse(request.method);
  switch (method) {
    case "session/new":
      createdSessions += 1;
      agent.respond(request, { sessionId: `fixture-session-${createdSessions}` });
      break;
    case "session/load":
    case "session/resume":
    case "session/close":
      agent.respond(request, {});
      break;
  }
}
agent.close();
