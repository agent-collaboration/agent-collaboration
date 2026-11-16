import assert from "node:assert/strict";
import { existsSync, watch, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { field } from "../exact-json.ts";
import { requestParameters, requireArgument, wireJson } from "./acp-stdio.ts";
import { AcpExchange } from "./acp-exchange.ts";

// The "send-race" scenario answers every request instead of following a
// script, records every frame it reads to <receipt> (argv 3), and changes the
// turn only at the points its behaviours name, so a test can force a send
// race. The first prompt's turn runs until the file <receipt>.release exists;
// a steer is injected while a turn runs and answered promptRequired otherwise;
// later prompts end at once; session/cancel ends a running turn as cancelled.
//
// argv after the receipt, any order:
//   no-steering: initialize does not advertise steering.
//   first-steer-ends-turn: the first steer ends the running turn first, then
//     is answered promptRequired.
//   first-steer-unknown: the first steer ends the running turn first, then is
//     answered with an outcome no client knows.
//   first-steer-before-turn-end: the first steer is answered promptRequired,
//     as by an agent whose turn ended, while the turn's own end still waits
//     for <receipt>.release.
//   second-steer-unknown: the second steer is answered with an outcome no
//     client knows, while the running turn keeps running.
//   hold-effort-change: sessions offer an effort setting, and a change to it
//     is answered only when the file <receipt>.setting-release exists.
//   first-turn-ends-max-tokens: the first prompt's turn, when <receipt>.release
//     ends it, ends with the stop reason max_tokens instead of end_turn, so a
//     test can tell that turn's end from any later turn's.
//   hold-each-prompt: instead of the first prompt only, every prompt's turn
//     runs until the file <receipt>.release-<n> exists, where n counts prompts
//     from 1; a steer is injected into the latest held turn while it runs.
//   replay-on-load: initialize advertises session/load, and a load replays one
//     old turn of the named session (a user message and its answer) before it
//     is answered.
//   first-effort-change-unconfirmed: sessions offer an effort setting; the
//     first change to it is answered with effort unchanged, as by an agent
//     that did not apply it, and later changes apply at once.
const sendRaceBehaviourNames = [
  "no-steering",
  "first-steer-ends-turn",
  "first-steer-unknown",
  "first-steer-before-turn-end",
  "second-steer-unknown",
  "hold-effort-change",
  "first-turn-ends-max-tokens",
  "hold-each-prompt",
  "replay-on-load",
  "first-effort-change-unconfirmed",
] as const;
type SendRaceBehaviour = (typeof sendRaceBehaviourNames)[number];

function sendRaceBehaviours(names: readonly string[]): ReadonlySet<SendRaceBehaviour> {
  const behaviours = new Set<SendRaceBehaviour>();
  for (const name of names) {
    const known = sendRaceBehaviourNames.find((candidate) => candidate === name);
    if (known === undefined) throw new Error(`unknown send-race behaviour ${name}`);
    behaviours.add(known);
  }
  return behaviours;
}

/**
 * Resolves once the test creates the file at `path`.
 * Woken by a watch on the file's directory; the existence check runs only
 * after the watch is registered, so a file written before the watch began is
 * still seen.
 */
function waitForFile(path: string): Promise<void> {
  return new Promise<void>((resolve, reject): void => {
    const watcher = watch(dirname(path));
    const releaseIfPresent = (): void => {
      if (!existsSync(path)) return;
      watcher.close();
      resolve();
    };
    watcher.on("change", releaseIfPresent);
    watcher.on("error", (error: Error): void => {
      watcher.close();
      reject(error);
    });
    releaseIfPresent();
  });
}

/** How the first prompt's turn ends once <receipt>.release exists. */
function releasedStopReason(behaviours: ReadonlySet<SendRaceBehaviour>): string {
  return behaviours.has("first-turn-ends-max-tokens") ? "max_tokens" : "end_turn";
}

function effortOptions(current: string): Record<string, unknown>[] {
  return [
    {
      id: "effort",
      name: "effort",
      category: "thought_level",
      type: "select",
      currentValue: current,
      options: ["medium", "high"].map((value) => ({ value, name: value })),
    },
  ];
}

function methodNotFound(agent: AcpExchange, request: Record<string, unknown>): void {
  if (request.id !== undefined)
    agent.send({
      jsonrpc: "2.0",
      id: request.id,
      error: { code: -32601n, message: "method not found" },
    });
}

/** replay-on-load: replays one old turn of the loaded session, then answers. */
function replayOldTurn(agent: AcpExchange, request: Record<string, unknown>): void {
  const sessionId = String(requestParameters(request).sessionId);
  for (const [sessionUpdate, text] of [
    ["user_message_chunk", "an old question"],
    ["agent_message_chunk", "an old answer"],
  ])
    agent.update({ sessionUpdate, content: { type: "text", text } }, sessionId);
  agent.respond(request, {});
}

/** The session effort, and how many changes to it the agent has answered. */
class EffortSetting {
  value = "medium";
  private changes = 0;

  /** Answers one change: held, unconfirmed or applied as the behaviours say. */
  async change(
    behaviours: ReadonlySet<SendRaceBehaviour>,
    receiptPath: string,
    value: string,
  ): Promise<void> {
    if (behaviours.has("hold-effort-change")) {
      await waitForFile(`${receiptPath}.setting-release`);
      this.value = value;
    } else if (behaviours.has("first-effort-change-unconfirmed")) {
      this.changes += 1;
      if (this.changes > 1) this.value = value;
    }
  }
}

async function initializeSendRace(
  agent: AcpExchange,
  behaviours: ReadonlySet<SendRaceBehaviour>,
): Promise<void> {
  await agent.initialize("send-race-fixture", {
    agentCapabilities: {
      promptCapabilities: { image: true },
      ...(behaviours.has("replay-on-load") ? { loadSession: true } : {}),
    },
    ...(behaviours.has("no-steering") ? {} : { _meta: { steering: { supported: true } } }),
  });
}

async function serveSendRace(receiptPath: string, names: readonly string[]): Promise<void> {
  const behaviours = sendRaceBehaviours(names);
  const frames: Record<string, unknown>[] = [];
  const agent = new AcpExchange((frame): void => {
    frames.push(frame);
    writeFileSync(receiptPath, wireJson(frames));
  });
  await initializeSendRace(agent, behaviours);
  const offersEffort =
    behaviours.has("hold-effort-change") || behaviours.has("first-effort-change-unconfirmed");
  let sessions = 0;
  let prompts = 0;
  let steers = 0;
  const effort = new EffortSetting();
  /** The first prompt, while its turn runs. */
  let running: Record<string, unknown> | undefined;
  /** hold-each-prompt: the prompts whose turns still run. */
  const heldTurns = new Set<Record<string, unknown>>();
  /** The agent said its turn ended while the turn's own end is still held. */
  let saidEnded = false;
  const endRunning = (stopReason: string): void => {
    const ended = running;
    running = undefined;
    if (ended !== undefined) {
      heldTurns.delete(ended);
      agent.endTurn(ended, stopReason);
    }
  };
  for (;;) {
    const request = await agent.readRequest();
    if (request === undefined) break;
    switch (request.method) {
      case "session/new":
        sessions += 1;
        agent.respond(request, {
          sessionId: sessions === 1 ? "fixture-session" : `fixture-session-${sessions}`,
          ...(offersEffort ? { configOptions: effortOptions(effort.value) } : {}),
        });
        break;
      case "session/load":
        if (behaviours.has("replay-on-load")) replayOldTurn(agent, request);
        else methodNotFound(agent, request);
        break;
      case "session/prompt": {
        prompts += 1;
        if (behaviours.has("hold-each-prompt")) {
          const held = request;
          heldTurns.add(held);
          running = held;
          void waitForFile(`${receiptPath}.release-${prompts}`).then((): void => {
            if (!heldTurns.delete(held)) return;
            if (running === held) running = undefined;
            agent.endTurn(held);
          });
          break;
        }
        if (prompts !== 1) {
          agent.endTurn(request);
          break;
        }
        const first = request;
        running = first;
        void waitForFile(`${receiptPath}.release`).then((): void => {
          if (running === first) endRunning(releasedStopReason(behaviours));
        });
        break;
      }
      case "_session/steering":
        steers += 1;
        if (steers === 1 && behaviours.has("first-steer-ends-turn")) {
          endRunning("end_turn");
          agent.respond(request, { outcome: "promptRequired" });
        } else if (steers === 1 && behaviours.has("first-steer-unknown")) {
          endRunning("end_turn");
          agent.respond(request, { outcome: "notAnOutcome" });
        } else if (steers === 1 && behaviours.has("first-steer-before-turn-end")) {
          saidEnded = true;
          agent.respond(request, { outcome: "promptRequired" });
        } else if (steers === 2 && behaviours.has("second-steer-unknown"))
          agent.respond(request, { outcome: "notAnOutcome" });
        else
          agent.respond(request, {
            outcome: running === undefined || saidEnded ? "promptRequired" : "injected",
          });
        break;
      case "session/cancel":
        endRunning("cancelled");
        break;
      case "session/set_config_option": {
        const parameters = requestParameters(request);
        if (parameters.configId === "effort")
          await effort.change(behaviours, receiptPath, String(parameters.value));
        agent.respond(request, { configOptions: effortOptions(effort.value) });
        break;
      }
      default:
        methodNotFound(agent, request);
    }
  }
  agent.close();
}

const scenario = requireArgument(2);
if (scenario === "send-race") await serveSendRace(requireArgument(3), process.argv.slice(4));
else {
  const agent = new AcpExchange();
  const name =
    scenario === "injected"
      ? "input-fixture"
      : scenario === "started"
        ? "started-steer-fixture"
        : "prompt-required-fixture";
  await agent.initialize(name, {
    agentCapabilities: { promptCapabilities: { image: true } },
    _meta: { steering: { supported: true } },
  });
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
}
