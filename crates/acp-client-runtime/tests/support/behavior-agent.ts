import { appendFileSync, existsSync, writeFileSync } from "node:fs";
import { pbkdf2 } from "node:crypto";
import { isAbsolute } from "node:path";
import { AcpExchange } from "../../../../verification/fixtures/acp-exchange.ts";
import { field, textField } from "../../../../verification/exact-json.ts";
import { listenForHeldRelease } from "../../../../verification/fixtures/held-release-control.ts";
import { requireArgument } from "../../../../verification/fixtures/acp-stdio.ts";
const receipt = requireArgument(2);
const compactCase = process.argv[3] ?? "manual";
const lifecycleProfile = compactCase === "lifecycle";
const primaryEnter = [
  "primary-enter",
  "primary-enter-select",
  "primary-enter-select-offered",
].includes(compactCase);
// Host fixtures explicitly select Cursor authentication; direct runtime behaviors retain theirs.
const authenticationMethod = process.argv[4] === undefined ? undefined : requireArgument(4);
if (authenticationMethod !== undefined && authenticationMethod !== "cursor_login")
  throw new Error("unsupported fixture authentication method");
const requiresConnectionAuthentication = authenticationMethod !== undefined;
// Optional private release socket: all controllers name the held Session owner.
const releaseSocket = process.argv[5] === undefined ? undefined : requireArgument(5);
if (releaseSocket !== undefined && !isAbsolute(releaseSocket))
  throw new Error("release socket must be absolute");
writeFileSync(receipt, "");
const agent = new AcpExchange();
await agent.initialize("behavior-fixture", {
  agentCapabilities: {
    loadSession: true,
    ...(lifecycleProfile ? { sessionCapabilities: { resume: {}, close: {} } } : {}),
    promptCapabilities: { image: true },
    _meta: { steering: { supported: true } },
  },
  ...(requiresConnectionAuthentication
    ? { authMethods: [{ id: "cursor_login", name: "Cursor login" }] }
    : {}),
});
if (requiresConnectionAuthentication) {
  const authentication = await agent.read("authenticate");
  if (field(authentication.params, "methodId") !== "cursor_login")
    throw new Error("unexpected initialization authentication method");
  agent.respond(authentication, {});
}
let sequence = 0;
let held: Record<string, unknown> | undefined;
function recordReleaseDiagnostic(
  event: string,
  details: Readonly<Record<string, unknown>> = {},
): void {
  if (process.env.BEHAVIOR_PEER_RELEASE_DIAGNOSTICS === "1")
    appendFileSync(
      `${receipt}.release-diagnostics.jsonl`,
      `${JSON.stringify({ event, atMs: Date.now(), ...details })}\n`,
    );
}
interface RecordedMessage {
  readonly sessionUpdate: "user_message_chunk" | "agent_message_chunk";
  readonly content: unknown;
}
interface KnownFixtureSession {
  readonly cwd: string;
  phase: "open" | "closed";
  readonly history: RecordedMessage[];
}
const knownSessions = new Map<string, KnownFixtureSession>();
function readSessionText(params: unknown, name: string): string | undefined {
  try {
    const value = textField(params, name);
    return value.length > 0 ? value : undefined;
  } catch {
    return undefined;
  }
}
function refuseSessionRequest(request: Readonly<Record<string, unknown>>): void {
  if (request.id !== undefined)
    agent.send({
      jsonrpc: "2.0",
      id: request.id,
      error: { code: -32602, message: "invalid fixture Session input" },
    });
}
function publishUpdate(update: Readonly<Record<string, unknown>>, sessionId: string): void {
  const sessionUpdate = update.sessionUpdate;
  if (
    lifecycleProfile &&
    (sessionUpdate === "user_message_chunk" || sessionUpdate === "agent_message_chunk")
  ) {
    const session = knownSessions.get(sessionId);
    if (session === undefined) throw new Error("observed message has no known Session");
    session.history.push({ sessionUpdate, content: update.content });
  }
  agent.update(update, sessionId);
}
function replayRecordedMessages(session: KnownFixtureSession, sessionId: string): void {
  for (const message of session.history) agent.update(message, sessionId);
}
function finishHeld(stopReason: "end_turn" | "cancelled"): void {
  const request = held;
  if (request === undefined) return;
  // Claim the request before emitting anything: release/cancel/close can settle it only once.
  held = undefined;
  recordReleaseDiagnostic("held-settled", { stopReason });
  if (stopReason === "end_turn") {
    const sessionId = field(request.params, "sessionId");
    if (typeof sessionId !== "string") throw new Error("held sessionId required");
    appendFileSync(receipt, `${JSON.stringify({ kind: "release", sessionId })}\n`);
    publishUpdate(
      { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "after" } },
      sessionId,
    );
  }
  agent.endTurn(request, stopReason);
  recordReleaseDiagnostic("terminal-sent", { stopReason });
}
let currentModel = "old";
let currentEffort = primaryEnter ? "high" : "low";
const savedEfforts = new Map([
  ["old", "high"],
  ["new", "low"],
]);
function restoreSavedModelEffort(modelId: string): void {
  if (!primaryEnter) return;
  const saved = savedEfforts.get(modelId);
  if (saved === undefined) throw new Error("saved model effort missing");
  currentEffort = saved;
}
function savePrimaryEnterState(): void {
  if (primaryEnter)
    writeFileSync(
      `${receipt}.state.json`,
      JSON.stringify({
        model: currentModel,
        effort: currentEffort,
        savedEfforts: Object.fromEntries(savedEfforts),
      }),
    );
}
function settingsOptions(): unknown[] {
  return [
    {
      id: "model",
      name: "Model",
      category: "model",
      type: "select",
      currentValue: currentModel,
      options: (compactCase === "model-catalog" ? ["old", "new", "rejected"] : ["old", "new"]).map(
        (value) => ({ value, name: value }),
      ),
    },
    {
      id: "effort",
      name: "Effort",
      category: "thought_level",
      type: "select",
      currentValue: currentEffort,
      options: (compactCase === "primary-enter-select" && currentModel === "new"
        ? ["low"]
        : primaryEnter || currentModel === "new"
          ? ["low", "high"]
          : ["low"]
      ).map((value) => ({
        value,
        name: value,
      })),
    },
  ];
}
function queueReleasePoolContention(): void {
  if (process.env.BEHAVIOR_PEER_RELEASE_POOL_CONTENTION !== "1") return;
  recordReleaseDiagnostic("release-pool-contention-start");
  pbkdf2("fixture", "fixture", 1_000, 32, "sha256", () => {
    recordReleaseDiagnostic("release-pool-contention-finished");
  });
}
if (releaseSocket !== undefined && existsSync(releaseSocket))
  throw new Error("release socket already exists before fixture startup");
const releaseControl =
  releaseSocket === undefined
    ? undefined
    : await listenForHeldRelease({
        socketPath: releaseSocket,
        releaseOwner(ownerId): boolean {
          const request = held;
          if (request === undefined || field(request.params, "sessionId") !== ownerId) return false;
          finishHeld("end_turn");
          return true;
        },
        recordReply(reply): void {
          appendFileSync(
            receipt,
            JSON.stringify({
              kind: "release-control",
              outcome: reply.kind,
              ownerId: reply.ownerId,
            }) + "\n",
          );
          recordReleaseDiagnostic("release-control-reply", reply);
        },
      });
recordReleaseDiagnostic("release-control-ready");
for (;;) {
  const request = await agent.readRequest();
  if (!request) break;
  const params = request.params;
  if (lifecycleProfile && String(request.method).startsWith("session/"))
    appendFileSync(receipt, `${JSON.stringify(request)}\n`);
  if (
    lifecycleProfile &&
    [
      "session/prompt",
      "session/cancel",
      "session/close",
      "session/resume",
      "session/load",
    ].includes(String(request.method))
  ) {
    const sessionId = readSessionText(params, "sessionId");
    const session = sessionId === undefined ? undefined : knownSessions.get(sessionId);
    if (sessionId === undefined || session === undefined) {
      refuseSessionRequest(request);
      continue;
    }
    if (request.method === "session/cancel" || request.method === "session/close") {
      if (held !== undefined && field(held.params, "sessionId") === sessionId)
        finishHeld("cancelled");
      if (request.method === "session/close") {
        session.phase = "closed";
        agent.respond(request, {});
      }
      continue;
    }
    if (request.method === "session/resume" || request.method === "session/load") {
      const cwd = readSessionText(params, "cwd");
      if (
        cwd === undefined ||
        !isAbsolute(cwd) ||
        cwd !== session.cwd ||
        (held !== undefined && field(held.params, "sessionId") === sessionId)
      ) {
        refuseSessionRequest(request);
        continue;
      }
      session.phase = "open";
      // Replay only this Session's actual messages; replay itself is not new history.
      if (request.method === "session/load") replayRecordedMessages(session, sessionId);
      agent.respond(request, {});
      continue;
    }
    if (
      session.phase === "closed" ||
      (held !== undefined && field(held.params, "sessionId") === sessionId)
    ) {
      refuseSessionRequest(request);
      continue;
    }
  }
  if (request.method === "cursor/list_available_models") {
    if (compactCase === "settings") {
      agent.respond(request, {
        models: ["old", "new"].map((value) => ({
          value,
          name: value,
          configOptions: [
            {
              id: "effort",
              name: "Effort",
              category: "thought_level",
              type: "select",
              currentValue: "low",
              options: (value === "old" ? ["low"] : ["low", "high"]).map((level) => ({
                value: level,
                name: level,
              })),
            },
          ],
        })),
      });
      continue;
    }
    if (primaryEnter) {
      agent.respond(request, {
        models: ["old", "new"].map((value) => ({
          value,
          name: value,
          configOptions:
            compactCase === "primary-enter"
              ? []
              : [
                  {
                    id: "effort",
                    name: "Effort",
                    category: "thought_level",
                    type: "select",
                    currentValue: value === "old" ? "high" : "low",
                    options: (value === "new" && compactCase === "primary-enter-select"
                      ? ["low"]
                      : ["low", "high"]
                    ).map((effort) => ({ value: effort, name: effort })),
                  },
                ],
        })),
      });
      continue;
    }
    if (compactCase === "held-model-catalog") {
      appendFileSync(receipt, `${JSON.stringify(request)}\n`);
      // Leave only this optional request unanswered; keep serving the same wire.
      continue;
    }
    if (compactCase === "model-catalog") {
      appendFileSync(receipt, `${JSON.stringify(request)}\n`);
      agent.respond(request, {
        models: [
          { value: "old", name: "Old", configOptions: [] },
          {
            value: "new",
            name: "New",
            configOptions: [
              {
                id: "effort",
                name: "Effort",
                category: "thought_level",
                type: "select",
                currentValue: "high",
                options: [
                  { value: "low", name: "Low" },
                  { value: "high", name: "High" },
                ],
              },
            ],
          },
          { value: "foreign", name: "Foreign", configOptions: [] },
        ],
      });
    } else agent.respond(request, {});
  } else if (request.method === "session/new") {
    const cwd = lifecycleProfile ? readSessionText(params, "cwd") : undefined;
    if (lifecycleProfile && (cwd === undefined || !isAbsolute(cwd))) {
      refuseSessionRequest(request);
      continue;
    }
    sequence += 1;
    const sessionId = `session-${sequence}`;
    if (cwd !== undefined) knownSessions.set(sessionId, { cwd, phase: "open", history: [] });
    savePrimaryEnterState();
    agent.respond(request, {
      sessionId,
      ...(["settings", "model-catalog", "held-model-catalog"].includes(compactCase) || primaryEnter
        ? { configOptions: settingsOptions() }
        : {}),
    });
    publishUpdate(
      {
        sessionUpdate: "available_commands_update",
        availableCommands: [{ name: "review", description: "Review (skill)" }],
      },
      sessionId,
    );
  } else if (
    request.method === "session/set_config_option" &&
    (["settings", "model-catalog", "held-model-catalog"].includes(compactCase) || primaryEnter)
  ) {
    if (!lifecycleProfile) appendFileSync(receipt, `${JSON.stringify(request)}\n`);
    const configId = field(params, "configId");
    const value = field(params, "value");
    if (compactCase === "model-catalog" && configId === "model" && value === "rejected") {
      agent.send({
        jsonrpc: "2.0",
        id: request.id,
        error: { code: -32602, message: "scripted model rejection" },
      });
      continue;
    }
    if (configId === "model" && (value === "old" || value === "new")) {
      currentModel = value;
      restoreSavedModelEffort(value);
    } else if (
      configId === "effort" &&
      (value === "low" ||
        (value === "high" && currentModel === "new" && compactCase !== "primary-enter-select"))
    ) {
      currentEffort = value;
      if (primaryEnter) savedEfforts.set(currentModel, value);
    } else throw new Error("unexpected or stale setting write");
    savePrimaryEnterState();
    agent.respond(request, { configOptions: settingsOptions() });
  } else if (request.method === "session/prompt") {
    savePrimaryEnterState();
    if (!lifecycleProfile) appendFileSync(receipt, `${JSON.stringify(request)}\n`);
    const sessionId = field(params, "sessionId");
    if (typeof sessionId !== "string") throw new Error("sessionId required");
    const prompt = field(params, "prompt");
    const first: unknown = Array.isArray(prompt) ? prompt[0] : undefined;
    const text = field(first, "text");
    if (lifecycleProfile) {
      if (text === "hold" && held !== undefined) {
        refuseSessionRequest(request);
        continue;
      }
      const session = knownSessions.get(sessionId);
      if (session === undefined || !Array.isArray(prompt))
        throw new Error("admitted fixture Session prompt missing");
      for (const content of prompt)
        session.history.push({ sessionUpdate: "user_message_chunk", content });
    }
    if (text === "hold") {
      held = request;
      queueReleasePoolContention();
      publishUpdate(
        { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "HOLDING" } },
        sessionId,
      );
      continue;
    }
    publishUpdate(
      { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "before" } },
      sessionId,
    );
    if (text === "semantic-error") {
      publishUpdate(
        {
          sessionUpdate: "tool_call",
          toolCallId: "semantic-tool",
          title: "Compact",
          kind: "think",
          status: "completed",
          _meta: { hint: "malformed" },
        },
        sessionId,
      );
    }
    if (text === "remove-skill")
      publishUpdate(
        { sessionUpdate: "available_commands_update", availableCommands: [] },
        sessionId,
      );
    if (text === "/compact" && compactCase !== "none") {
      const hint =
        compactCase === "auto" ? "automatic" : compactCase === "malformed" ? "malformed" : "manual";
      if (compactCase !== "terminal")
        publishUpdate(
          {
            sessionUpdate: "tool_call",
            toolCallId: "compact-tool",
            title: "Compact",
            kind: "think",
            status: "in_progress",
            _meta: { hint },
          },
          sessionId,
        );
      publishUpdate(
        {
          sessionUpdate: compactCase === "terminal" ? "tool_call" : "tool_call_update",
          toolCallId: "compact-tool",
          title: "Compact",
          kind: "other",
          status: compactCase === "failed" ? "failed" : "completed",
          ...(compactCase === "terminal" ? { _meta: { hint } } : {}),
        },
        sessionId,
      );
    }
    publishUpdate(
      { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "after" } },
      sessionId,
    );
    agent.endTurn(
      request,
      text === "/compact" && compactCase === "cancelled" ? "cancelled" : "end_turn",
    );
  } else if (request.method === "session/cancel") {
    if (releaseSocket !== undefined) appendFileSync(receipt, `${JSON.stringify(request)}\n`);
    if (held === undefined) {
      if (request.id !== undefined) agent.respond(request, {});
      continue;
    }
    if (compactCase === "remove-on-cancel") {
      const sessionId = field(held.params, "sessionId");
      if (typeof sessionId !== "string") throw new Error("held sessionId required");
      agent.update(
        { sessionUpdate: "available_commands_update", availableCommands: [] },
        sessionId,
      );
    }
    finishHeld("cancelled");
  } else if (request.method === "session/close") {
    if (
      releaseSocket !== undefined &&
      held !== undefined &&
      field(params, "sessionId") === field(held.params, "sessionId")
    )
      finishHeld("cancelled");
    agent.respond(request, {});
  } else if (request.id !== undefined) agent.respond(request, {});
}
if (releaseControl !== undefined) {
  releaseControl.close();
}
