import { appendFileSync, existsSync, unwatchFile, watchFile, writeFileSync } from "node:fs";
import { isAbsolute } from "node:path";
import { AcpExchange } from "../../../../verification/fixtures/acp-exchange.ts";
import { field, textField } from "../../../../verification/exact-json.ts";
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
// Optional owned sentinel: legacy callers retain their existing held/cancel behavior and receipts.
const releaseFile = process.argv[5] === undefined ? undefined : requireArgument(5);
if (releaseFile !== undefined && !isAbsolute(releaseFile))
  throw new Error("release sentinel must be absolute");
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
function stopReleaseWatch(): void {
  if (releaseFile !== undefined) unwatchFile(releaseFile, releaseHeldIfSignalled);
}
function finishHeld(stopReason: "end_turn" | "cancelled"): void {
  const request = held;
  if (request === undefined) return;
  // Claim the request before emitting anything: release/cancel/close can settle it only once.
  held = undefined;
  stopReleaseWatch();
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
}
function releaseHeldIfSignalled(): void {
  if (releaseFile !== undefined && existsSync(releaseFile)) finishHeld("end_turn");
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
      if (releaseFile !== undefined && existsSync(releaseFile))
        throw new Error("release sentinel already exists before held prompt");
      held = request;
      publishUpdate(
        { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "HOLDING" } },
        sessionId,
      );
      if (releaseFile !== undefined) {
        // Stat polling works under the test runner's descriptor limit; the owner bounds the gate.
        watchFile(releaseFile, { interval: 20 }, releaseHeldIfSignalled);
        // Close the exists/watch race without polling or a wall-clock delay.
        releaseHeldIfSignalled();
      }
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
    if (releaseFile !== undefined) appendFileSync(receipt, `${JSON.stringify(request)}\n`);
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
      releaseFile !== undefined &&
      held !== undefined &&
      field(params, "sessionId") === field(held.params, "sessionId")
    )
      finishHeld("cancelled");
    agent.respond(request, {});
  } else if (request.id !== undefined) agent.respond(request, {});
}
stopReleaseWatch();
