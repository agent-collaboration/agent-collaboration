import assert from "node:assert/strict";
import { existsSync, writeFileSync } from "node:fs";
import { setTimeout as delay } from "node:timers/promises";
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
//   hold-effort-change: sessions offer an effort setting, and a change to it
//     is answered only when the file <receipt>.setting-release exists.
const sendRaceBehaviourNames = [
  "no-steering",
  "first-steer-ends-turn",
  "first-steer-unknown",
  "first-steer-before-turn-end",
  "hold-effort-change",
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

/** Waits until the test creates the file at `path`. */
async function waitForFile(path: string): Promise<void> {
  while (!existsSync(path)) await delay(10);
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

async function serveSendRace(receiptPath: string, names: readonly string[]): Promise<void> {
  const behaviours = sendRaceBehaviours(names);
  const frames: Record<string, unknown>[] = [];
  const agent = new AcpExchange((frame): void => {
    frames.push(frame);
    writeFileSync(receiptPath, wireJson(frames));
  });
  await agent.initialize("send-race-fixture", {
    agentCapabilities: { promptCapabilities: { image: true } },
    ...(behaviours.has("no-steering") ? {} : { _meta: { steering: { supported: true } } }),
  });
  let sessions = 0;
  let prompts = 0;
  let steers = 0;
  let effort = "medium";
  /** The first prompt, while its turn runs. */
  let running: Record<string, unknown> | undefined;
  /** The agent said its turn ended while the turn's own end is still held. */
  let saidEnded = false;
  const endRunning = (stopReason: string): void => {
    const ended = running;
    running = undefined;
    if (ended !== undefined) agent.endTurn(ended, stopReason);
  };
  for (;;) {
    const request = await agent.readRequest();
    if (request === undefined) break;
    switch (request.method) {
      case "session/new":
        sessions += 1;
        agent.respond(request, {
          sessionId: sessions === 1 ? "fixture-session" : `fixture-session-${sessions}`,
          ...(behaviours.has("hold-effort-change") ? { configOptions: effortOptions(effort) } : {}),
        });
        break;
      case "session/prompt": {
        prompts += 1;
        if (prompts !== 1) {
          agent.endTurn(request);
          break;
        }
        const first = request;
        running = first;
        void waitForFile(`${receiptPath}.release`).then((): void => {
          if (running === first) endRunning("end_turn");
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
        } else
          agent.respond(request, {
            outcome: running === undefined || saidEnded ? "promptRequired" : "injected",
          });
        break;
      case "session/cancel":
        endRunning("cancelled");
        break;
      case "session/set_config_option": {
        const parameters = requestParameters(request);
        if (behaviours.has("hold-effort-change") && parameters.configId === "effort") {
          await waitForFile(`${receiptPath}.setting-release`);
          effort = String(parameters.value);
        }
        agent.respond(request, { configOptions: effortOptions(effort) });
        break;
      }
      default:
        if (request.id !== undefined)
          agent.send({
            jsonrpc: "2.0",
            id: request.id,
            error: { code: -32601n, message: "method not found" },
          });
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
