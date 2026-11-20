import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { AcpStdio, requestParameters, requireArgument, wireJson } from "./acp-stdio.ts";
import { fileAppears, startReleaseBarrier } from "./release-barrier.ts";

interface ConfigOption {
  readonly id: string;
  readonly name: string;
  readonly category: string;
  readonly type: "select" | "boolean";
  readonly currentValue: string | boolean;
  readonly options?: readonly { readonly value: string; readonly name: string }[];
}

function option(
  configId: string,
  category: string,
  values: readonly string[],
  currentValue: string,
): ConfigOption {
  return {
    id: configId,
    name: configId,
    category,
    type: "select",
    currentValue,
    options: values.map((value) => ({ value, name: value })),
  };
}

interface CursorSettings {
  mode: string;
  model: string;
  thinking: string;
  context: string;
  effort: string;
  fast: string;
}

function catalogOptions(
  scenario: string,
  current: CursorSettings,
  effortId: string,
): ConfigOption[] {
  let models = ["fixture-a", "fixture-b"];
  let efforts = ["medium", "high"];
  if (scenario === "dynamic" && current.model === "fixture-a") {
    efforts = ["medium"];
    if (current.mode === "agent") models = ["fixture-a"];
  }
  const result = [
    option("mode", "mode", ["agent", "ask"], current.mode),
    option("model", "model", models, current.model),
    option("thinking", "thought_level", ["false", "true"], current.thinking),
    option("context", "model_config", ["300k", "1m"], current.context),
  ];
  if (scenario !== "thinking-only")
    result.push(option(effortId, "thought_level", efforts, current.effort));
  if (["ambiguous", "explicit-wins", "duplicate-explicit"].includes(scenario))
    result.push(
      option(
        scenario === "duplicate-explicit" ? "effort" : "reasoning",
        "thought_level",
        ["medium", "high"],
        "medium",
      ),
    );
  if (scenario === "duplicate-mixed")
    result.push({
      id: "effort",
      name: "effort",
      category: "thought_level",
      type: "boolean",
      currentValue: true,
    });
  result.push(option("fast", "model_config", ["false", "true"], current.fast));
  return result;
}

async function serveSharedSettings(peer: {
  readonly current: CursorSettings;
  readonly readRequest: () => Promise<Record<string, unknown> | undefined>;
  readonly reply: (request: Record<string, unknown>, result: unknown) => void;
}): Promise<void> {
  const sessions = new Set<string>();
  for (;;) {
    const request = await peer.readRequest();
    if (request === undefined) return;
    if (request.method === "session/new") {
      const sessionId = `shared-session-${sessions.size + 1}`;
      sessions.add(sessionId);
      peer.reply(request, {
        sessionId,
        configOptions: catalogOptions("shared-settings", peer.current, "effort"),
      });
      continue;
    }
    assert.equal(request.method, "session/set_config_option");
    const parameters = requestParameters(request);
    assert.ok(typeof parameters.sessionId === "string" && sessions.has(parameters.sessionId));
    if (parameters.configId === "model") {
      assert.ok(parameters.value === "fixture-a" || parameters.value === "fixture-b");
      peer.current.model = parameters.value;
    } else {
      assert.equal(parameters.configId, "effort");
      assert.ok(parameters.value === "medium" || parameters.value === "high");
      peer.current.effort = parameters.value;
    }
    // Cursor's shared manager changes the value, without notifying the other session.
    peer.reply(request, {
      configOptions: catalogOptions("shared-settings", peer.current, "effort"),
    });
  }
}

// The "reactive" scenario answers every request instead of following a
// script, so a test can drive any order of creates, setting changes and
// prompts. It starts like Grok 4.6: mode agent, model fixture-a, effort medium
// and fast on, and turns fast back on whenever the model changes.
//
// argv after the receipt, any order:
//   boolean-fast: fast is a boolean option instead of a false/true select.
//   on-off-fast: use the on/off select identifiers advertised by Claude.
//   fast-not-offered-on-fixture-b: omit the fast option on fixture-b.
//   refuse-fast-once: the first fast change is answered with an error.
//   refuse-second-fast: the second fast change is answered with an error.
//   refuse-fast-after-model: fast changes are accepted until a model change is
//     accepted after the first fast change; the next fast change is refused,
//     later ones accepted.
//   malformed-fast-reply: the first fast change gets a success with no
//     configOptions; fast stays as it was.
//   malformed-fast-after-model: as refuse-fast-after-model, but that one fast
//     change gets a success with no configOptions instead of an error.
//   fast-stays-on: the first fast change is answered with a valid catalog in
//     which fast still reads on.
//   fast-option-vanishes: the first fast change is answered with a valid
//     catalog that has no fast option.
//   hold-first-prompt: the first prompt's turn stays running until the file
//     <receipt>.release exists, so a test can send to a busy session.
//   hold-first-prompt-open: as hold-first-prompt, but the agent keeps
//     answering other requests while the turn runs, and a session/cancel
//     ends the held turn with stopReason cancelled.
//   advertise-close: initialize advertises session/close.
//   advertise-steering: initialize advertises steering; _session/steering is
//     answered injected while a held-open turn runs, promptRequired otherwise.
//   prompt-directives: "ask-permission" asks for a fetch, expects the refusal,
//     and waits for session/cancel before ending its turn; one whose text is
//     "unknown-stop" ends with the stop reason "future_reason"; one whose
//     text is "hold-open" stays running, as hold-first-prompt-open, until a
//     session/cancel ends it.
//   exit-on-stop-file: the agent exits once the file <receipt>.stop exists.
//   permission-after-steer: with advertise-steering, a steer answered injected
//     is followed by a permission request for a fetch in the running turn;
//     the client's answer is recorded and the turn keeps running.
//   held-turn-ends-max-tokens: with hold-first-prompt-open, the released
//     held turn ends with the stop reason max_tokens instead of end_turn, so
//     a test can tell that turn's end from any later turn's.
//   hold-load: session/load is answered only once the file
//     <receipt>.release-load exists; other requests are answered meanwhile.
const reactiveBehaviourNames = [
  "boolean-fast",
  "on-off-fast",
  "fast-not-offered-on-fixture-b",
  "refuse-fast-once",
  "refuse-second-fast",
  "refuse-fast-after-model",
  "malformed-fast-reply",
  "malformed-fast-after-model",
  "fast-stays-on",
  "fast-option-vanishes",
  "hold-first-prompt",
  "hold-first-prompt-open",
  "advertise-close",
  "advertise-steering",
  "prompt-directives",
  "exit-on-stop-file",
  "permission-after-steer",
  "held-turn-ends-max-tokens",
  "hold-load",
] as const;
type ReactiveBehaviour = (typeof reactiveBehaviourNames)[number];

function reactiveBehaviours(names: readonly string[]): ReadonlySet<ReactiveBehaviour> {
  const behaviours = new Set<ReactiveBehaviour>();
  for (const name of names) {
    const known = reactiveBehaviourNames.find((candidate) => candidate === name);
    if (known === undefined) throw new Error(`unknown reactive behaviour ${name}`);
    behaviours.add(known);
  }
  return behaviours;
}

interface ReactiveSession {
  mode: string;
  model: string;
  effort: string;
  fast: boolean;
}

type FastReply = "apply" | "refuse" | "malformed" | "stays-on" | "vanishes";

function reactiveOptions(
  session: ReactiveSession,
  behaviours: ReadonlySet<ReactiveBehaviour>,
  withFast: boolean,
): ConfigOption[] {
  const result = [
    option("mode", "mode", ["agent", "ask"], session.mode),
    option("model", "model", ["fixture-a", "fixture-b"], session.model),
    option("effort", "thought_level", ["medium", "high"], session.effort),
  ];
  if (
    !withFast ||
    (behaviours.has("fast-not-offered-on-fixture-b") && session.model === "fixture-b")
  )
    return result;
  if (behaviours.has("boolean-fast"))
    result.push({
      id: "fast",
      name: "fast",
      category: "model_config",
      type: "boolean",
      currentValue: session.fast,
    });
  else if (behaviours.has("on-off-fast"))
    result.push(option("fast", "model_config", ["on", "off"], session.fast ? "on" : "off"));
  else result.push(option("fast", "model_config", ["false", "true"], String(session.fast)));
  return result;
}

interface ReactivePeer {
  readonly behaviours: ReadonlySet<ReactiveBehaviour>;
  /** The file whose appearance ends a held first prompt's turn. */
  readonly releasePath: string;
  /** The file whose appearance ends the agent (exit-on-stop-file). */
  readonly stopPath: string;
  readonly readRequest: () => Promise<Record<string, unknown> | undefined>;
  readonly reply: (request: Record<string, unknown>, result: unknown) => void;
  readonly refuse: (request: Record<string, unknown>, message: string) => void;
  readonly send: (message: Record<string, unknown>) => void;
}

/** Decides how each fast change is answered, from the behaviours and history. */
class ReactiveFastReplies {
  private fastChanges = 0;
  private modelChangedSinceFast = false;
  private afterModelFastFailed = false;

  private readonly behaviours: ReadonlySet<ReactiveBehaviour>;

  constructor(behaviours: ReadonlySet<ReactiveBehaviour>) {
    this.behaviours = behaviours;
  }

  // A model change counts only after a fast change was answered, so the
  // create's own model change doesn't trip the after-model behaviours.
  modelChanged(): void {
    if (this.fastChanges > 0) this.modelChangedSinceFast = true;
  }

  next(): FastReply {
    this.fastChanges += 1;
    const afterModel = this.modelChangedSinceFast && !this.afterModelFastFailed;
    this.modelChangedSinceFast = false;
    if (afterModel && this.behaviours.has("refuse-fast-after-model")) {
      this.afterModelFastFailed = true;
      return "refuse";
    }
    if (afterModel && this.behaviours.has("malformed-fast-after-model")) {
      this.afterModelFastFailed = true;
      return "malformed";
    }
    if (this.fastChanges === 2 && this.behaviours.has("refuse-second-fast")) return "refuse";
    if (this.fastChanges !== 1) return "apply";
    if (this.behaviours.has("refuse-fast-once")) return "refuse";
    if (this.behaviours.has("malformed-fast-reply")) return "malformed";
    if (this.behaviours.has("fast-stays-on")) return "stays-on";
    if (this.behaviours.has("fast-option-vanishes")) return "vanishes";
    return "apply";
  }
}

function answerReactiveFast(
  peer: ReactivePeer,
  session: ReactiveSession,
  request: Record<string, unknown>,
  replies: ReactiveFastReplies,
): void {
  const parameters = requestParameters(request);
  const off = peer.behaviours.has("boolean-fast")
    ? parameters.type === "boolean" && parameters.value === false
    : parameters.value === (peer.behaviours.has("on-off-fast") ? "off" : "false");
  if (!off) {
    peer.refuse(request, "fixture accepts only fast off");
    return;
  }
  const reply = replies.next();
  if (reply === "refuse") {
    peer.refuse(request, "fast change refused");
    return;
  }
  if (reply === "malformed") {
    peer.reply(request, {});
    return;
  }
  if (reply === "apply") session.fast = false;
  peer.reply(request, {
    configOptions: reactiveOptions(session, peer.behaviours, reply !== "vanishes"),
  });
}

function answerReactiveSetting(
  peer: ReactivePeer,
  session: ReactiveSession | undefined,
  request: Record<string, unknown>,
  replies: ReactiveFastReplies,
): void {
  const parameters = requestParameters(request);
  if (session === undefined) {
    peer.refuse(request, "unknown session");
    return;
  }
  if (parameters.configId === "fast") {
    answerReactiveFast(peer, session, request, replies);
    return;
  }
  const value = String(parameters.value);
  if (parameters.configId === "mode" && ["agent", "ask"].includes(value)) session.mode = value;
  else if (parameters.configId === "model" && ["fixture-a", "fixture-b"].includes(value)) {
    session.model = value;
    // Cursor turns fast back on whenever the model changes.
    session.fast = true;
    replies.modelChanged();
  } else if (parameters.configId === "effort" && ["medium", "high"].includes(value))
    session.effort = value;
  else {
    peer.refuse(request, "unknown setting");
    return;
  }
  peer.reply(request, { configOptions: reactiveOptions(session, peer.behaviours, true) });
}

/** Exits the agent once `stopPath` exists (exit-on-stop-file). */
function exitWhenFileAppears(stopPath: string): void {
  void fileAppears(stopPath).then((): never => process.exit(0));
}

/** The prompt's first text block, when it has one. */
function promptText(request: Record<string, unknown>): string | undefined {
  const blocks = requestParameters(request).prompt;
  if (!Array.isArray(blocks)) return undefined;
  const first: unknown = blocks[0];
  if (typeof first !== "object" || first === null || !("text" in first)) return undefined;
  return typeof first.text === "string" ? first.text : undefined;
}

let permissionRequests = 0;

/** Answers a directed prompt or returns the request that is waiting for cancel. */
async function answerDirectedPrompt(
  peer: ReactivePeer,
  request: Record<string, unknown>,
): Promise<Record<string, unknown> | undefined> {
  const text = peer.behaviours.has("prompt-directives") ? promptText(request) : undefined;
  if (text === "ask-permission") {
    permissionRequests += 1;
    const callbackId = `permission-${permissionRequests}`;
    peer.send({
      jsonrpc: "2.0",
      id: callbackId,
      method: "session/request_permission",
      params: {
        sessionId: requestParameters(request).sessionId,
        toolCall: {
          toolCallId: `fetch-${permissionRequests}`,
          title: "Fetch a page",
          kind: "fetch",
        },
        options: [
          { optionId: "allow", name: "Allow", kind: "allow_once" },
          { optionId: "reject", name: "Reject", kind: "reject_once" },
        ],
      },
    });
    const refused = await peer.readRequest();
    assert.equal(refused?.id, callbackId);
    assert.deepEqual(refused?.result, { outcome: { outcome: "cancelled" } });
    return request;
  }
  peer.reply(request, { stopReason: text === "unknown-stop" ? "future_reason" : "end_turn" });
  return undefined;
}

let steerPermissionRequests = 0;

/** Asks a permission in the running turn without waiting for the answer
 * (permission-after-steer); the answer arrives as a later frame. */
function askPermissionAfterSteer(peer: ReactivePeer, running: Record<string, unknown>): void {
  steerPermissionRequests += 1;
  peer.send({
    jsonrpc: "2.0",
    id: `steer-permission-${steerPermissionRequests}`,
    method: "session/request_permission",
    params: {
      sessionId: requestParameters(running).sessionId,
      toolCall: {
        toolCallId: `steer-fetch-${steerPermissionRequests}`,
        title: "Fetch a page",
        kind: "fetch",
      },
      options: [
        { optionId: "allow", name: "Allow", kind: "allow_once" },
        { optionId: "reject", name: "Reject", kind: "reject_once" },
      ],
    },
  });
}

function methodNotFound(peer: ReactivePeer, request: Record<string, unknown>): void {
  peer.send({
    jsonrpc: "2.0",
    id: request.id,
    error: { code: -32601n, message: "method not found" },
  });
}

async function serveReactiveSettings(peer: ReactivePeer): Promise<void> {
  let promptsSeen = 0;
  /** The held-open prompt, while its turn runs. */
  let heldOpen: Record<string, unknown> | undefined;
  if (peer.behaviours.has("exit-on-stop-file")) exitWhenFileAppears(peer.stopPath);
  const sessions = new Map<string, ReactiveSession>();
  const replies = new ReactiveFastReplies(peer.behaviours);
  for (;;) {
    const request = await peer.readRequest();
    if (request === undefined) return;
    // A permission answer is a response, recorded by readRequest; nothing to answer.
    if (request.method === undefined && peer.behaviours.has("permission-after-steer")) continue;
    switch (request.method) {
      case "session/new": {
        const sessionId = `reactive-session-${sessions.size + 1}`;
        const session = { mode: "agent", model: "fixture-a", effort: "medium", fast: true };
        sessions.set(sessionId, session);
        peer.reply(request, {
          sessionId,
          configOptions: reactiveOptions(session, peer.behaviours, true),
        });
        break;
      }
      case "session/load": {
        // A restarted agent loads a stored session at its defaults.
        const sessionId = String(requestParameters(request).sessionId);
        const session = sessions.get(sessionId) ?? {
          mode: "agent",
          model: "fixture-a",
          effort: "medium",
          fast: true,
        };
        sessions.set(sessionId, session);
        const answer = (): void =>
          peer.reply(request, { configOptions: reactiveOptions(session, peer.behaviours, true) });
        if (peer.behaviours.has("hold-load"))
          void fileAppears(`${peer.releasePath}-load`).then(answer);
        else answer();
        break;
      }
      case "session/set_config_option":
        answerReactiveSetting(
          peer,
          sessions.get(String(requestParameters(request).sessionId)),
          request,
          replies,
        );
        break;
      case "session/prompt":
        promptsSeen += 1;
        if (promptsSeen === 1 && peer.behaviours.has("hold-first-prompt-open")) {
          const held = request;
          heldOpen = held;
          void fileAppears(peer.releasePath).then((): void => {
            if (heldOpen !== held) return;
            heldOpen = undefined;
            peer.reply(held, {
              stopReason: peer.behaviours.has("held-turn-ends-max-tokens")
                ? "max_tokens"
                : "end_turn",
            });
          });
          break;
        }
        if (promptsSeen === 1 && peer.behaviours.has("hold-first-prompt"))
          await fileAppears(peer.releasePath);
        if (peer.behaviours.has("prompt-directives") && promptText(request) === "hold-open") {
          heldOpen = request;
          break;
        }
        heldOpen = (await answerDirectedPrompt(peer, request)) ?? heldOpen;
        break;
      case "session/cancel":
        if (heldOpen !== undefined) {
          const held = heldOpen;
          heldOpen = undefined;
          peer.reply(held, { stopReason: "cancelled" });
        } else methodNotFound(peer, request);
        break;
      case "_session/steering":
        if (!peer.behaviours.has("advertise-steering")) methodNotFound(peer, request);
        else
          peer.reply(request, { outcome: heldOpen === undefined ? "promptRequired" : "injected" });
        if (
          heldOpen !== undefined &&
          peer.behaviours.has("advertise-steering") &&
          peer.behaviours.has("permission-after-steer")
        )
          askPermissionAfterSteer(peer, heldOpen);
        break;
      case "authenticate":
        peer.reply(request, {});
        break;
      case "session/close":
        sessions.delete(String(requestParameters(request).sessionId));
        peer.reply(request, {});
        break;
      default:
        methodNotFound(peer, request);
    }
  }
}

function applyScriptedSetting(
  current: CursorSettings,
  configId: "mode" | "model" | "effort" | "budget",
  value: string,
): void {
  if (configId === "mode") current.mode = value;
  else if (configId === "model") {
    current.model = value;
    // Cursor turns fast back on whenever the model changes.
    current.fast = "true";
  } else current.effort = value;
}

/** The initialize answer; the reactive scenario also serves the Host's real
 * Cursor binding, which needs Cursor's login method. */
function initializeResult(
  scenario: string,
  behaviours: ReadonlySet<ReactiveBehaviour>,
): Record<string, unknown> {
  return {
    protocolVersion: 1n,
    agentCapabilities: {
      loadSession: true,
      mcpCapabilities: { http: true },
      ...(behaviours.has("advertise-close") ? { sessionCapabilities: { close: {} } } : {}),
    },
    agentInfo: { name: "cursor-settings-wire-peer", version: "1" },
    ...(scenario === "reactive"
      ? { authMethods: [{ id: "cursor_login", name: "Cursor login" }] }
      : {}),
    ...(behaviours.has("advertise-steering") ? { _meta: { steering: { supported: true } } } : {}),
  };
}

/** The behaviours argv names; only the reactive scenario takes any. */
function scenarioBehaviours(scenario: string): ReadonlySet<ReactiveBehaviour> {
  return scenario === "reactive" ? reactiveBehaviours(process.argv.slice(4)) : new Set();
}

function reactivePeer(
  io: AcpStdio,
  readRequest: () => Promise<Record<string, unknown> | undefined>,
  reply: (request: Record<string, unknown>, result: unknown) => void,
): ReactivePeer {
  startReleaseBarrier(requireArgument(3));
  return {
    behaviours: reactiveBehaviours(process.argv.slice(4)),
    releasePath: `${requireArgument(3)}.release`,
    stopPath: `${requireArgument(3)}.stop`,
    readRequest,
    reply,
    refuse: (request: Record<string, unknown>, message: string): void => {
      io.send({ jsonrpc: "2.0", id: request.id, error: { code: -32000n, message } });
    },
    send: (message: Record<string, unknown>): void => {
      io.send(message);
    },
  };
}

export async function main(): Promise<void> {
  const scenario = requireArgument(2);
  const receiptPath = requireArgument(3);
  const requests: Record<string, unknown>[] = [];
  const current = {
    mode: "agent",
    model: "fixture-a",
    thinking: "true",
    context: "300k",
    effort: "medium",
    fast: "false",
  };
  let effortId: "effort" | "budget" = "effort";
  const io = new AcpStdio();
  async function readRequest(): Promise<Record<string, unknown> | undefined> {
    const request = await io.readRequest();
    if (request !== undefined) {
      requests.push(request);
      writeFileSync(receiptPath, wireJson(requests));
    }
    return request;
  }
  const reply = (request: Record<string, unknown>, result: unknown): void => {
    io.send({ jsonrpc: "2.0", id: request.id, result });
  };
  try {
    const initialize = await readRequest();
    assert.equal(initialize?.method, "initialize");
    if (initialize === undefined) throw new Error("initialize request required");
    reply(initialize, initializeResult(scenario, scenarioBehaviours(scenario)));
    if (scenario === "negotiation") {
      await io.drain();
      return;
    }
    if (scenario === "shared-settings")
      return await serveSharedSettings({ current, readRequest, reply });
    if (scenario === "reactive")
      return await serveReactiveSettings(reactivePeer(io, readRequest, reply));
    const create = await readRequest();
    assert.ok(create?.method === "session/new" || create?.method === "session/load");
    if (scenario === "fallback" || scenario === "ambiguous") effortId = "budget";
    if (create.method === "session/new")
      reply(create, {
        sessionId: "fixture-session",
        configOptions: catalogOptions(scenario, current, effortId),
      });
    else {
      reply(create, { configOptions: catalogOptions(scenario, current, effortId) });
      effortId = "budget";
      io.send({
        jsonrpc: "2.0",
        method: "session/update",
        params: {
          sessionId: "fixture-session",
          update: {
            sessionUpdate: "config_option_update",
            configOptions: catalogOptions(scenario, current, effortId),
          },
        },
      });
    }
    const expectedIds: readonly ("mode" | "model" | "effort" | "budget")[] =
      scenario === "dynamic" ? ["mode", "model", "effort"] : [effortId];
    for (const expectedId of expectedIds) {
      const request = await readRequest();
      if (request === undefined) return;
      assert.equal(request.method, "session/set_config_option");
      if (
        ["ambiguous", "thinking-only", "duplicate-explicit", "duplicate-mixed"].includes(scenario)
      ) {
        io.send({
          jsonrpc: "2.0",
          id: request.id,
          error: { code: -32602n, message: "unexpected mutation: ambiguous or absent effort" },
        });
        await io.drain();
        return;
      }
      const parameters = requestParameters(request);
      assert.equal(parameters.configId, expectedId);
      const expectedValue =
        expectedId === "mode" ? "ask" : expectedId === "model" ? "fixture-b" : "high";
      assert.equal(parameters.value, expectedValue);
      applyScriptedSetting(current, expectedId, expectedValue);
      if (scenario === "adjusted") {
        current.context = "1m";
        current.fast = "true";
      }
      reply(request, { configOptions: catalogOptions(scenario, current, effortId) });
    }
    await io.drain();
  } finally {
    io.close();
  }
}

if (import.meta.main)
  main().catch((error: unknown): void => {
    console.error(error);
    process.exitCode = 1;
  });
