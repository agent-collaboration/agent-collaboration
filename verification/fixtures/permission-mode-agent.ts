// An ACP agent whose permission modes are Claude's or Cursor's, for the
// Router's auto-mode plan (D1, R3-R5). It records every session/new working
// set, every mode it was asked to switch to and the answer to each permission
// request it raises (a prompt's text is the ACP tool call to ask about). It
// rewrites the receipt before answering each request, so a client that has its
// answer can read the receipt, and stays alive until stdin closes.
//
// argv: <receipt> <claude|cursor> <grants-directories|no-directories> [clamp-auto]
//   clamp-auto: a requested "auto" lands on "acceptEdits", as claude-agent-acp
//   0.81.0 does for a model without auto support.
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { selectOption } from "./provider-settings-options.ts";

const receipt = requireArgument(2);
const profile = requireArgument(3);
const directories = requireArgument(4);
const clampAuto = process.argv[5] === "clamp-auto";

const profiles: Record<string, { readonly start: string; readonly modes: readonly string[] }> = {
  claude: { start: "default", modes: ["default", "acceptEdits", "plan", "auto"] },
  cursor: { start: "ask", modes: ["agent", "plan", "ask"] },
};
const chosen = profiles[profile];
if (chosen === undefined) throw new Error(`unknown permission profile ${profile}`);

const agent = new AcpExchange();
await agent.initialize(`${profile}-permission-fixture`, {
  authMethods: [{ id: "cursor_login", name: "Cursor login" }],
  ...(directories === "grants-directories"
    ? { agentCapabilities: { sessionCapabilities: { additionalDirectories: {} } } }
    : {}),
});

interface Observed {
  readonly sessions: { sessionId: string; cwd: unknown; additionalDirectories: unknown }[];
  readonly modeRequests: { sessionId: unknown; value: unknown }[];
  readonly permissionAnswers: { sessionId: string; answer: unknown }[];
}
const observed: Observed = { sessions: [], modeRequests: [], permissionAnswers: [] };
let permissionRequestId = 900;
const currentMode = new Map<string, string>();
const modeOption = (sessionId: string): Record<string, unknown> =>
  selectOption({
    id: "mode",
    current: currentMode.get(sessionId) ?? chosen.start,
    values: chosen.modes,
  });

for (;;) {
  const request = await agent.readRequest();
  if (request === undefined) break;
  switch (request.method) {
    case "session/new": {
      const sessionId = `fixture-session-${observed.sessions.length + 1}`;
      const params = field(request, "params") as Record<string, unknown>;
      observed.sessions.push({
        sessionId,
        cwd: params.cwd,
        additionalDirectories: params.additionalDirectories ?? null,
      });
      currentMode.set(sessionId, chosen.start);
      agent.receipt(receipt, observed);
      agent.respond(request, { sessionId, configOptions: [modeOption(sessionId)] });
      break;
    }
    case "session/set_config_option": {
      const sessionId = String(field(request, "params", "sessionId"));
      const value = String(field(request, "params", "value"));
      observed.modeRequests.push({ sessionId, value });
      currentMode.set(sessionId, clampAuto && value === "auto" ? "acceptEdits" : value);
      agent.receipt(receipt, observed);
      agent.respond(request, { configOptions: [modeOption(sessionId)] });
      break;
    }
    case "session/prompt": {
      // The prompt text is the tool call to ask permission for, as ACP JSON.
      const sessionId = String(field(request, "params", "sessionId"));
      const blocks = field(request, "params", "prompt") as Record<string, unknown>[];
      const toolCall = JSON.parse(String(blocks[0]?.text)) as Record<string, unknown>;
      permissionRequestId += 1;
      agent.send({
        jsonrpc: "2.0",
        id: permissionRequestId,
        method: "session/request_permission",
        params: {
          sessionId,
          toolCall: {
            toolCallId: `tool-${permissionRequestId}`,
            title: "Fixture action",
            ...toolCall,
          },
          options: [
            { optionId: "allow", name: "Allow", kind: "allow_once" },
            { optionId: "reject", name: "Reject", kind: "reject_once" },
          ],
        },
      });
      const answer = await agent.read();
      observed.permissionAnswers.push({ sessionId, answer: field(answer, "result") });
      agent.receipt(receipt, observed);
      agent.endTurn(request);
      break;
    }
    case "authenticate":
    case "session/close":
      agent.respond(request, {});
      break;
    default:
      agent.send({
        jsonrpc: "2.0",
        id: request.id,
        error: { code: -32601, message: "method not found" },
      });
  }
}
agent.close();
