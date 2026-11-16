/**
 * Recorded-frame fixtures for the O5 comparison: a wire builder and the comparison script's shape
 * (send, steer mid-turn, `/model`) with switches that seed one defect at a time. Used by the
 * evaluation tests and the compare command's tests; never by a run.
 */
import type { WireFrame } from "./turn-skeleton.ts";

/** Builds one connection's frames in wire order. */
export class Wire {
  private readonly frames: WireFrame[] = [];
  private nextRequest = 100;

  private push(direction: WireFrame["direction"], message: unknown): this {
    this.frames.push({ seq: this.frames.length, connection: 0, direction, message });
    return this;
  }
  /** The real-Codex reference's own launch: a thread/start answered with the script's thread. */
  launch(): this {
    const id = this.nextRequest++;
    return this.push("cliToServer", {
      id,
      method: "thread/start",
      params: { threadSource: "user" },
    }).push("serverToCli", { id, result: { thread: { id: "t" } } });
  }
  start(clientId: string): this {
    return this.push("cliToServer", {
      id: this.nextRequest++,
      method: "turn/start",
      params: { threadId: "t", input: [], clientUserMessageId: clientId },
    });
  }
  steer(clientId: string, turnId: string): this {
    return this.push("cliToServer", {
      id: this.nextRequest++,
      method: "turn/steer",
      params: { threadId: "t", expectedTurnId: turnId, input: [], clientUserMessageId: clientId },
    });
  }
  settingsRequest(id: number): this {
    return this.push("cliToServer", {
      id,
      method: "thread/settings/update",
      params: { threadId: "t" },
    });
  }
  settingsReply(id: number, model: string, effort: string | null, tier: string | null): this {
    return this.push("serverToCli", {
      id,
      result: { threadSettings: { model, effort, serviceTier: tier } },
    });
  }
  turn(phase: "started" | "completed", turnId: string): this {
    return this.push("serverToCli", {
      method: `turn/${phase}`,
      params: {
        threadId: "t",
        turn: { id: turnId, status: phase === "started" ? "inProgress" : "completed" },
      },
    });
  }
  item(
    phase: "started" | "completed",
    turnId: string,
    id: string,
    type: string,
    clientId?: string,
  ): this {
    return this.push("serverToCli", {
      method: `item/${phase}`,
      params: {
        threadId: "t",
        turnId,
        item: { id, type, ...(clientId === undefined ? {} : { clientId }) },
      },
    });
  }
  user(turnId: string, id: string, clientId: string): this {
    return this.item("started", turnId, id, "userMessage", clientId).item(
      "completed",
      turnId,
      id,
      "userMessage",
      clientId,
    );
  }
  agent(turnId: string, id: string): this {
    return this.item("started", turnId, id, "agentMessage")
      .push("serverToCli", {
        method: "item/agentMessage/delta",
        params: { threadId: "t", turnId, itemId: id, delta: "x" },
      })
      .item("completed", turnId, id, "agentMessage");
  }
  usage(turnId: string, used: number, window: number): this {
    const total = { totalTokens: used };
    return this.push("serverToCli", {
      method: "thread/tokenUsage/updated",
      params: {
        threadId: "t",
        turnId,
        tokenUsage: { total, last: total, modelContextWindow: window },
      },
    });
  }
  build(): readonly WireFrame[] {
    return this.frames;
  }
}

export const SCRIPTED = { model: "gpt-x", effort: "low", service_tier: null };

export interface ScriptRunOptions {
  readonly turnId: string;
  readonly window: number;
  readonly dropSteerReceipt?: boolean;
  readonly duplicateSteerReceipt?: boolean;
  readonly agentBeforeUser?: boolean;
  readonly settings?: readonly [string, string | null, string | null];
  /** The real-Codex reference opens its own thread first (assignment Correction 29). */
  readonly referenceLaunch?: boolean;
}

/** The compare script on one target: send, steer mid-turn, then `/model`. */
export function scriptRun(options: ScriptRunOptions): Wire {
  const opened = new Wire();
  if (options.referenceLaunch === true) opened.launch();
  const wire = opened.start("c1").turn("started", options.turnId);
  if (options.agentBeforeUser === true) wire.agent(options.turnId, "early");
  wire.user(options.turnId, "u1", "c1").agent(options.turnId, "a1").steer("c2", options.turnId);
  if (options.dropSteerReceipt !== true) wire.user(options.turnId, "u2", "c2");
  if (options.duplicateSteerReceipt === true) wire.user(options.turnId, "u2b", "c2");
  wire
    .agent(options.turnId, "a2")
    .usage(options.turnId, 4000, options.window)
    .turn("completed", options.turnId);
  const [model, effort, tier] = options.settings ?? [
    SCRIPTED.model,
    SCRIPTED.effort,
    SCRIPTED.service_tier,
  ];
  return wire.settingsRequest(500).settingsReply(500, model, effort, tier);
}
