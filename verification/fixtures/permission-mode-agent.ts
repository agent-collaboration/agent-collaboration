// An ACP agent whose permission modes are Claude's or Cursor's, for the
// Router's auto-mode plan (D1, R3-R5). It records every session/new working
// set, every mode it was asked to switch to and the answer to each permission
// request it raises (a prompt's text is the ACP tool call to ask about). It
// rewrites the receipt before answering each request, so a client that has its
// answer can read the receipt, and stays alive until stdin closes.
//
// argv: <receipt> <profile> <grants-directories|no-directories> [behaviour...]
//   profile: claude (default, acceptEdits, plan, auto), claude-without-auto
//   (no auto), claude-without-auto-or-accept-edits (default, plan) or cursor.
//   Behaviours, any order:
//   clamp-auto: a requested "auto" lands on "acceptEdits", as claude-agent-acp
//     0.81.0 does for a model without auto support.
//   reject-mode: every mode change is answered with an error.
//   plain-prompts: record and end prompts without asking for permission.
//   silent-prompts: record prompts but send no response, even after cancel.
//   late-cancel-confirmation: end the first held prompt only when the test releases cancel.
//   refuse-prompt: record the prompt and reject its ACP request.
//   silent-first-restore: leave the first load or resume unanswered.
//   disconnect-during-restore: exit after reading the load or resume.
//   restore-session-not-found, restore-other-session-not-found, restore-invalid-cwd:
//     reject load or resume with Cursor's Invalid params envelope.
//   malformed-mode-reply: the first mode change gets a success with no
//     configOptions, which a client cannot read; the mode stays and later
//     changes are answered normally.
//   advertise-close: the agent advertises session/close.
//   hold-close-refusal: refuse a close after <receipt>.release-close appears.
//   offer-fast: sessions also offer a select option "fast" (false, true),
//     starting at "true".
//   refuse-fast-once: the first fast change is answered with an error.
//   advertise-restore: the agent advertises session/load and session/resume,
//     records each restore's additionalDirectories and _meta, and answers it
//     with the session in its profile's start mode.
//   restored-in-auto: with advertise-restore, a restored session is in auto.
//   leave-auto-after-first-edit: report default after the first permission reply.
//   restored-in-accept-edits: the restored mode is the provider's acceptEdits fallback.
//   narrow-restore-modes: after a restore, advertise only default and plan.
//   restore-fast-on: with advertise-restore and offer-fast, a restored
//     session reads fast on, as an agent that forgot its fast setting.
import { field } from "../exact-json.ts";
import { requireArgument } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";
import { fileAppears, startReleaseBarrier } from "./release-barrier.ts";
import { selectOption } from "./provider-settings-options.ts";

const receipt = requireArgument(2);
const profile = requireArgument(3);
const directories = requireArgument(4);
const behaviours = new Set(process.argv.slice(5));
const known = [
  "clamp-auto",
  "reject-mode",
  "plain-prompts",
  "silent-prompts",
  "late-cancel-confirmation",
  "refuse-prompt",
  "silent-first-restore",
  "disconnect-during-restore",
  "restore-session-not-found",
  "restore-other-session-not-found",
  "restore-invalid-cwd",
  "malformed-mode-reply",
  "advertise-close",
  "hold-close-refusal",
  "offer-fast",
  "refuse-fast-once",
  "advertise-restore",
  "restored-in-auto",
  "leave-auto-after-first-edit",
  "restored-in-accept-edits",
  "narrow-restore-modes",
  "restore-fast-on",
];
for (const behaviour of behaviours)
  if (!known.includes(behaviour)) throw new Error(`unknown fixture behaviour ${behaviour}`);
const clampAuto = behaviours.has("clamp-auto");
if (behaviours.has("hold-close-refusal") || behaviours.has("late-cancel-confirmation"))
  startReleaseBarrier(receipt);

const profiles: Record<string, { readonly start: string; readonly modes: readonly string[] }> = {
  claude: { start: "default", modes: ["default", "acceptEdits", "plan", "auto"] },
  "claude-without-auto": { start: "default", modes: ["default", "acceptEdits", "plan"] },
  "claude-without-auto-or-accept-edits": { start: "default", modes: ["default", "plan"] },
  cursor: { start: "ask", modes: ["agent", "plan", "ask"] },
};
const chosen = profiles[profile];
if (chosen === undefined) throw new Error(`unknown permission profile ${profile}`);

const agent = new AcpExchange();
await agent.initialize(`${profile}-permission-fixture`, {
  authMethods: [{ id: "cursor_login", name: "Cursor login" }],
  agentCapabilities: {
    ...(behaviours.has("advertise-restore") ? { loadSession: true } : {}),
    sessionCapabilities: {
      ...(directories === "grants-directories" ? { additionalDirectories: {} } : {}),
      ...(behaviours.has("advertise-close") ? { close: {} } : {}),
      ...(behaviours.has("advertise-restore") ? { resume: {} } : {}),
    },
  },
});

interface Observed {
  readonly sessions: { sessionId: string; cwd: unknown; additionalDirectories: unknown }[];
  readonly modeRequests: { sessionId: unknown; value: unknown }[];
  readonly fastRequests: { sessionId: unknown; value: unknown }[];
  readonly prompts: string[];
  readonly closes: string[];
  readonly cancels: string[];
  readonly restores: {
    method: string;
    sessionId: string;
    additionalDirectories: unknown;
    meta: unknown;
  }[];
  readonly permissionAnswers: { sessionId: string; answer: unknown }[];
}
const observed: Observed = {
  sessions: [],
  modeRequests: [],
  fastRequests: [],
  prompts: [],
  closes: [],
  cancels: [],
  restores: [],
  permissionAnswers: [],
};
let permissionRequestId = 900;
const currentMode = new Map<string, string>();
const narrowedModeCatalogues = new Set<string>();
const modeOption = (sessionId: string): Record<string, unknown> =>
  selectOption({
    id: "mode",
    current: currentMode.get(sessionId) ?? chosen.start,
    values: narrowedModeCatalogues.has(sessionId) ? ["default", "plan"] : chosen.modes,
  });
const currentFast = new Map<string, string>();
const configOptions = (sessionId: string): Record<string, unknown>[] => [
  modeOption(sessionId),
  ...(behaviours.has("offer-fast")
    ? [
        selectOption({
          id: "fast",
          category: "model_config",
          current: currentFast.get(sessionId) ?? "true",
          values: ["false", "true"],
        }),
      ]
    : []),
];
let modeChanges = 0;
let fastChanges = 0;
let heldPrompt: Record<string, unknown> | undefined;
const refuse = (request: Record<string, unknown>, message: string): void => {
  agent.send({ jsonrpc: "2.0", id: request.id, error: { code: -32000, message } });
};

requests: for (;;) {
  const request = await agent.readRequest();
  if (request === undefined) break;
  if (request.id === undefined && request.method !== "session/cancel") continue;
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
      agent.respond(request, { sessionId, configOptions: configOptions(sessionId) });
      break;
    }
    case "session/set_config_option": {
      const sessionId = String(field(request, "params", "sessionId"));
      const configId = String(field(request, "params", "configId"));
      const value = field(request, "params", "value");
      if (configId === "fast") {
        observed.fastRequests.push({ sessionId, value });
        agent.receipt(receipt, observed);
        fastChanges += 1;
        if (behaviours.has("refuse-fast-once") && fastChanges === 1) {
          refuse(request, "fast change refused");
          break;
        }
        currentFast.set(sessionId, String(value));
        agent.respond(request, { configOptions: configOptions(sessionId) });
        break;
      }
      observed.modeRequests.push({ sessionId, value });
      agent.receipt(receipt, observed);
      modeChanges += 1;
      if (behaviours.has("reject-mode")) {
        refuse(request, "mode change refused");
        break;
      }
      if (behaviours.has("malformed-mode-reply") && modeChanges === 1) {
        agent.respond(request, {});
        break;
      }
      const mode = String(value);
      currentMode.set(sessionId, clampAuto && mode === "auto" ? "acceptEdits" : mode);
      agent.respond(request, { configOptions: configOptions(sessionId) });
      break;
    }
    case "session/prompt": {
      // The prompt text is the tool call to ask permission for, as ACP JSON.
      const sessionId = String(field(request, "params", "sessionId"));
      observed.prompts.push(sessionId);
      if (behaviours.has("silent-prompts")) {
        agent.receipt(receipt, observed);
        break;
      }
      if (behaviours.has("late-cancel-confirmation") && observed.prompts.length === 1) {
        heldPrompt = request;
        agent.receipt(receipt, observed);
        break;
      }
      if (behaviours.has("refuse-prompt")) {
        agent.receipt(receipt, observed);
        agent.send({
          jsonrpc: "2.0",
          id: request.id,
          error: { code: -32041, message: "prompt response failed" },
        });
        break;
      }
      if (behaviours.has("plain-prompts")) {
        agent.receipt(receipt, observed);
        agent.endTurn(request);
        break;
      }
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
      if (
        behaviours.has("leave-auto-after-first-edit") &&
        observed.permissionAnswers.length === 1
      ) {
        currentMode.set(sessionId, "default");
        agent.send({
          jsonrpc: "2.0",
          method: "session/update",
          params: {
            sessionId,
            update: { sessionUpdate: "current_mode_update", currentModeId: "default" },
          },
        });
      }
      agent.endTurn(request);
      break;
    }
    case "session/load":
    case "session/resume": {
      const sessionId = String(field(request, "params", "sessionId"));
      const params = field(request, "params") as Record<string, unknown>;
      observed.restores.push({
        method: String(request.method),
        sessionId,
        additionalDirectories: params.additionalDirectories ?? null,
        meta: params._meta ?? null,
      });
      const restoredMode = behaviours.has("restored-in-accept-edits")
        ? "acceptEdits"
        : behaviours.has("restored-in-auto")
          ? "auto"
          : chosen.start;
      currentMode.set(sessionId, restoredMode);
      if (behaviours.has("narrow-restore-modes")) narrowedModeCatalogues.add(sessionId);
      if (behaviours.has("restore-fast-on")) currentFast.set(sessionId, "true");
      agent.receipt(receipt, observed);
      if (behaviours.has("disconnect-during-restore")) break requests;
      if (behaviours.has("silent-first-restore") && observed.restores.length === 1) break;
      const invalidParams = behaviours.has("restore-session-not-found")
        ? `Session "${sessionId}" not found`
        : behaviours.has("restore-other-session-not-found")
          ? `Session "${sessionId}-other" not found`
          : behaviours.has("restore-invalid-cwd")
            ? `Invalid working directory for session "${sessionId}"`
            : undefined;
      if (invalidParams !== undefined) {
        agent.send({
          jsonrpc: "2.0",
          id: request.id,
          error: { code: -32602, message: "Invalid params", data: { message: invalidParams } },
        });
        break;
      }
      agent.respond(request, { configOptions: configOptions(sessionId) });
      break;
    }
    case "session/cancel": {
      observed.cancels.push(String(field(request, "params", "sessionId")));
      agent.receipt(receipt, observed);
      if (heldPrompt !== undefined) {
        const prompt = heldPrompt;
        heldPrompt = undefined;
        void fileAppears(`${receipt}.release-cancel`).then((): void => {
          agent.endTurn(prompt, "cancelled");
        });
      }
      break;
    }
    case "session/close":
      observed.closes.push(String(field(request, "params", "sessionId")));
      agent.receipt(receipt, observed);
      if (behaviours.has("hold-close-refusal")) {
        void fileAppears(`${receipt}.release-close`).then((): void => {
          agent.send({
            jsonrpc: "2.0",
            id: request.id,
            error: { code: -32601, message: "close was refused" },
          });
        });
      } else agent.respond(request, {});
      break;
    case "authenticate":
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
