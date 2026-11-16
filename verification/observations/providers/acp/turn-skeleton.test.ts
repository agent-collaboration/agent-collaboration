import assert from "node:assert/strict";
import { test } from "node:test";

import {
  checkCrossRun,
  checkProviderUsage,
  checkRunRules,
  reduceTurnSkeleton,
} from "./turn-skeleton.ts";
import type { RunUnderComparison, SkeletonRule, WireFrame } from "./turn-skeleton.ts";

/** Builds one run's frames in wire order. */
class Wire {
  private readonly frames: WireFrame[] = [];
  private nextRequest = 100;

  private push(direction: WireFrame["direction"], message: unknown): this {
    this.frames.push({ seq: this.frames.length, connection: 0, direction, message });
    return this;
  }

  start(clientId: string | undefined): this {
    return this.push("cliToServer", {
      id: this.nextRequest++,
      method: "turn/start",
      params: {
        threadId: "t",
        input: [],
        ...(clientId === undefined ? {} : { clientUserMessageId: clientId }),
      },
    });
  }

  steer(clientId: string, turnId: string): this {
    return this.push("cliToServer", {
      id: this.nextRequest++,
      method: "turn/steer",
      params: { threadId: "t", expectedTurnId: turnId, input: [], clientUserMessageId: clientId },
    });
  }

  turnStarted(turnId: string): this {
    return this.push("serverToCli", {
      method: "turn/started",
      params: { threadId: "t", turn: { id: turnId, status: "inProgress" } },
    });
  }

  turnCompleted(turnId: string, status = "completed"): this {
    return this.push("serverToCli", {
      method: "turn/completed",
      params: { threadId: "t", turn: { id: turnId, status } },
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

  delta(turnId: string, itemId: string): this {
    return this.push("serverToCli", {
      method: "item/agentMessage/delta",
      params: { threadId: "t", turnId, itemId, delta: "x" },
    });
  }

  userItem(turnId: string, id: string, clientId: string): this {
    return this.item("started", turnId, id, "userMessage", clientId).item(
      "completed",
      turnId,
      id,
      "userMessage",
      clientId,
    );
  }

  agentItem(turnId: string, id: string): this {
    return this.item("started", turnId, id, "agentMessage")
      .delta(turnId, id)
      .item("completed", turnId, id, "agentMessage");
  }

  usage(turnId: string, used: number, window: number | null): this {
    const breakdown = { totalTokens: used };
    return this.push("serverToCli", {
      method: "thread/tokenUsage/updated",
      params: {
        threadId: "t",
        turnId,
        tokenUsage: { total: breakdown, last: breakdown, modelContextWindow: window },
      },
    });
  }

  approval(requestId: string, turnId: string, decision: string | undefined): this {
    this.push("serverToCli", {
      id: requestId,
      method: "item/commandExecution/requestApproval",
      params: { threadId: "t", turnId, itemId: "a" },
    });
    return decision === undefined
      ? this
      : this.push("cliToServer", { id: requestId, result: { decision } });
  }

  settings(model: string, effort: string | null, serviceTier: string | null): this {
    return this.push("serverToCli", {
      method: "thread/settings/updated",
      params: { threadId: "t", threadSettings: { model, effort, serviceTier } },
    });
  }

  /** A settings update that omits `serviceTier`: the tier it had is kept. */
  settingsWithoutTier(model: string, effort: string | null): this {
    return this.push("serverToCli", {
      method: "thread/settings/updated",
      params: { threadId: "t", threadSettings: { model, effort } },
    });
  }

  build(): readonly WireFrame[] {
    return this.frames;
  }
}

/** The action script's shape in a valid run: send, then steer mid-turn after earlier output. */
function validRun(agentItems: number): Wire {
  const wire = new Wire().start("c1").turnStarted("T1").userItem("T1", "u1", "c1");
  for (let index = 0; index < agentItems; index += 1) wire.agentItem("T1", `a${index}`);
  return wire
    .steer("c2", "T1")
    .userItem("T1", "u2", "c2")
    .agentItem("T1", "after")
    .usage("T1", 1234, 200_000);
}

function rulesOf(frames: readonly WireFrame[]): SkeletonRule[] {
  return checkRunRules(reduceTurnSkeleton(frames)).map((violation) => violation.rule);
}

test("a valid run with a late steer passes k1–k4", () => {
  assert.deepEqual(checkRunRules(reduceTurnSkeleton(validRun(1).turnCompleted("T1").build())), []);
});

test("reasoning items and extra agent items are allowed differences", () => {
  const frames = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .item("started", "T1", "r1", "reasoning")
    .item("completed", "T1", "r1", "reasoning")
    .agentItem("T1", "a1")
    .agentItem("T1", "a2")
    .turnCompleted("T1")
    .build();
  assert.deepEqual(rulesOf(frames), []);
});

test("k1: a missing completion, a second completion and an item after completion fail", () => {
  assert.deepEqual(rulesOf(validRun(1).build()), ["k1", "k3"]);
  assert.ok(rulesOf(validRun(1).turnCompleted("T1").turnCompleted("T1").build()).includes("k1"));
  assert.ok(
    rulesOf(validRun(1).turnCompleted("T1").agentItem("T1", "late").build()).includes("k1"),
  );
});

test("k2: agent item before the opening user item, a missing id and a duplicated id fail", () => {
  const early = new Wire()
    .start("c1")
    .turnStarted("T1")
    .agentItem("T1", "a1")
    .userItem("T1", "u1", "c1")
    .turnCompleted("T1")
    .build();
  assert.deepEqual(rulesOf(early), ["k2"]);

  const missingId = new Wire()
    .start(undefined)
    .turnStarted("T1")
    .agentItem("T1", "a1")
    .turnCompleted("T1")
    .build();
  assert.deepEqual(rulesOf(missingId), ["k2"]);

  const duplicated = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .userItem("T1", "u1b", "c1")
    .agentItem("T1", "a1")
    .turnCompleted("T1")
    .build();
  assert.deepEqual(rulesOf(duplicated), ["k2"]);
});

test("k3: a steer with no receipt, or a receipt after completion, fails", () => {
  const dropped = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .agentItem("T1", "a1")
    .steer("c2", "T1")
    .agentItem("T1", "a2")
    .turnCompleted("T1")
    .build();
  assert.deepEqual(rulesOf(dropped), ["k3"]);

  const lateReceipt = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .agentItem("T1", "a1")
    .steer("c2", "T1")
    .turnCompleted("T1")
    .userItem("T1", "u2", "c2")
    .build();
  assert.ok(rulesOf(lateReceipt).includes("k3"));
});

test("k4: a delta outside its item, a double completion and an empty answer fail", () => {
  const deltaFirst = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .delta("T1", "a1")
    .item("started", "T1", "a1", "agentMessage")
    .item("completed", "T1", "a1", "agentMessage")
    .turnCompleted("T1")
    .build();
  assert.deepEqual(rulesOf(deltaFirst), ["k4"]);

  const twice = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .agentItem("T1", "a1")
    .item("completed", "T1", "a1", "agentMessage")
    .turnCompleted("T1")
    .build();
  assert.deepEqual(rulesOf(twice), ["k4"]);

  const empty = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .turnCompleted("T1")
    .build();
  assert.deepEqual(rulesOf(empty), ["k4"]);
});

test("usage: the CLI must receive the provider's own size and used", () => {
  const skeleton = reduceTurnSkeleton(validRun(1).turnCompleted("T1").build());
  assert.deepEqual(checkProviderUsage(skeleton, [{ turnId: "T1", used: 1234, size: 200_000 }]), []);

  const window770k = reduceTurnSkeleton(
    new Wire().start("c1").turnStarted("T1").usage("T1", 1234, 770_000).build(),
  );
  assert.deepEqual(
    checkProviderUsage(window770k, [{ turnId: "T1", used: 1234, size: 1_000_000 }]).map(
      (v) => v.rule,
    ),
    ["usage"],
  );
  const wrongUsed = reduceTurnSkeleton(
    new Wire().start("c1").turnStarted("T1").usage("T1", 99, 200_000).build(),
  );
  assert.equal(
    checkProviderUsage(wrongUsed, [{ turnId: "T1", used: 1234, size: 200_000 }]).length,
    1,
  );
  const never = reduceTurnSkeleton(new Wire().start("c1").turnStarted("T1").build());
  assert.equal(checkProviderUsage(never, [{ turnId: "T1", used: 1, size: 2 }]).length, 1);
});

function comparison(
  frames: readonly WireFrame[],
  extra: Partial<RunUnderComparison> = {},
): RunUnderComparison {
  return {
    skeleton: reduceTurnSkeleton(frames),
    scriptedSettings: undefined,
    agentApprovalOutcomes: undefined,
    ...extra,
  };
}

test("k5: corresponding runs with different item counts and ids match", () => {
  const reference = comparison(validRun(1).turnCompleted("T1").build());
  const candidate = comparison(
    new Wire()
      .start("other-c1")
      .turnStarted("X1")
      .userItem("X1", "x", "other-c1")
      .agentItem("X1", "b1")
      .agentItem("X1", "b2")
      .steer("other-c2", "X1")
      .userItem("X1", "y", "other-c2")
      .agentItem("X1", "b3")
      .turnCompleted("X1")
      .build(),
  );
  assert.deepEqual(checkCrossRun(reference, candidate), []);
});

test("k5: a different turn status or turn count fails", () => {
  const reference = comparison(validRun(1).turnCompleted("T1").build());
  const interrupted = comparison(validRun(1).turnCompleted("T1", "interrupted").build());
  assert.deepEqual(
    checkCrossRun(reference, interrupted).map((v) => v.rule),
    ["k5"],
  );
});

test("k5: a lost model change fails against the script's pick for that target", () => {
  const picked = { model: "claude-haiku-4-5", effort: "low", serviceTier: null };
  const kept = comparison(
    validRun(1).turnCompleted("T1").settings("claude-haiku-4-5", "low", null).build(),
    {
      scriptedSettings: picked,
    },
  );
  const reference = comparison(
    validRun(1).turnCompleted("T1").settings("gpt-5.5", "low", null).build(),
    {
      scriptedSettings: { model: "gpt-5.5", effort: "low", serviceTier: null },
    },
  );
  assert.deepEqual(checkCrossRun(reference, kept), []);

  const lost = comparison(
    validRun(1).turnCompleted("T1").settings("provider-default", null, null).build(),
    {
      scriptedSettings: picked,
    },
  );
  assert.deepEqual(
    checkCrossRun(reference, lost).map((v) => v.rule),
    ["k5"],
  );
});

test("k5: an approval whose choice never reaches the agent fails", () => {
  const withApproval = (decision: string): readonly WireFrame[] =>
    new Wire()
      .start("c1")
      .turnStarted("T1")
      .userItem("T1", "u1", "c1")
      .approval("srv-1", "T1", decision)
      .agentItem("T1", "a1")
      .turnCompleted("T1")
      .build();
  const reference = comparison(withApproval("accept"), { agentApprovalOutcomes: ["accept"] });
  assert.deepEqual(
    checkCrossRun(
      reference,
      comparison(withApproval("accept"), { agentApprovalOutcomes: ["accept"] }),
    ),
    [],
  );
  assert.deepEqual(
    checkCrossRun(reference, comparison(withApproval("accept"), { agentApprovalOutcomes: [] })).map(
      (v) => v.rule,
    ),
    ["k5"],
  );
  const noPrompt = comparison(validRun(1).turnCompleted("T1").build());
  assert.ok(
    checkCrossRun(reference, noPrompt).some((v) => v.detail.startsWith("approval prompts")),
  );
});

test("k4: an item completed before it started fails even without deltas", () => {
  const reversed = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .item("completed", "T1", "a1", "agentMessage")
    .item("started", "T1", "a1", "agentMessage")
    .turnCompleted("T1")
    .build();
  assert.deepEqual(rulesOf(reversed), ["k4"]);
});

test("k5: approvals with unobserved agent outcomes are not a match", () => {
  const withApproval = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .approval("srv-1", "T1", "accept")
    .agentItem("T1", "a1")
    .turnCompleted("T1")
    .build();
  assert.deepEqual(
    checkCrossRun(comparison(withApproval), comparison(withApproval)).map((v) => v.rule),
    ["k5", "k5"],
  );
});

test("frames from two CLI connections are refused, not merged", () => {
  const frames = validRun(1).turnCompleted("T1").build();
  const second = frames.map((frame) => ({ ...frame, seq: frame.seq + 1000, connection: 1 }));
  assert.throws(() => reduceTurnSkeleton([...frames, ...second]), /several CLI connections/);
});

test("usage: a run with no provider report cannot pass", () => {
  const skeleton = reduceTurnSkeleton(validRun(1).turnCompleted("T1").build());
  assert.deepEqual(
    checkProviderUsage(skeleton, []).map((v) => v.rule),
    ["usage"],
  );
});

test("settings: an omitted tier keeps the previous one, an explicit null clears it (F6)", () => {
  const kept = reduceTurnSkeleton(
    new Wire().settings("gpt-x", "low", "priority").settingsWithoutTier("gpt-x", "high").build(),
  );
  assert.deepEqual(kept.lastSettings, { model: "gpt-x", effort: "high", serviceTier: "priority" });
  const cleared = reduceTurnSkeleton(
    new Wire().settings("gpt-x", "low", "priority").settings("gpt-x", "high", null).build(),
  );
  assert.deepEqual(cleared.lastSettings, { model: "gpt-x", effort: "high", serviceTier: null });
});

/**
 * The D0 r1 c52 steer turn on real Codex 0.166.0 (Correction 58): the in-flight agent message gets
 * no item/completed once the steer's user item starts; a new agent message carries the reply.
 */
function steeredOver(
  options: { readonly reply?: boolean; readonly lateDelta?: boolean } = {},
): Wire {
  const wire = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .item("started", "T1", "a0", "agentMessage")
    .delta("T1", "a0")
    .steer("c2", "T1")
    .userItem("T1", "u2", "c2");
  if (options.lateDelta === true) wire.delta("T1", "a0");
  if (options.reply !== false) wire.agentItem("T1", "after");
  return wire.usage("T1", 1234, 200_000);
}

const k4Of = (frames: readonly WireFrame[]): string[] =>
  checkRunRules(reduceTurnSkeleton(frames))
    .filter((violation) => violation.rule === "k4")
    .map((violation) => violation.detail);

test("k4: an agent message superseded by a steer may end uncompleted (the c52 shape, C58)", () => {
  assert.deepEqual(k4Of(steeredOver().turnCompleted("T1").build()), []);
  // A Router that does complete the superseded item also passes.
  const completed = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .item("started", "T1", "a0", "agentMessage")
    .delta("T1", "a0")
    .item("completed", "T1", "a0", "agentMessage")
    .steer("c2", "T1")
    .userItem("T1", "u2", "c2")
    .agentItem("T1", "after")
    .usage("T1", 1234, 200_000)
    .turnCompleted("T1");
  assert.deepEqual(k4Of(completed.build()), []);
});

test("k4 negatives: no steer, a delta after the steer, a double completion, a stray delta (C58)", () => {
  // An uncompleted agent message with no later steer.
  const unsteered = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .item("started", "T1", "a0", "agentMessage")
    .delta("T1", "a0")
    .agentItem("T1", "a1")
    .usage("T1", 1234, 200_000)
    .turnCompleted("T1");
  assert.ok(k4Of(unsteered.build()).includes("item a0: completed 0 times"));
  // The superseded item still streams after the steer's user item started.
  assert.ok(
    k4Of(steeredOver({ lateDelta: true }).turnCompleted("T1").build()).includes(
      "item a0: completed 0 times",
    ),
  );
  // Completed twice.
  const twice = validRun(1).item("completed", "T1", "a0", "agentMessage").turnCompleted("T1");
  assert.ok(k4Of(twice.build()).includes("item a0: completed 2 times"));
  // A delta before its item started.
  const stray = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .delta("T1", "a0")
    .item("started", "T1", "a0", "agentMessage")
    .item("completed", "T1", "a0", "agentMessage")
    .usage("T1", 1234, 200_000)
    .turnCompleted("T1");
  assert.ok(k4Of(stray.build()).some((detail) => detail.includes("before the item")));
});

test("k4: a turn whose only agent message was superseded, with none completed, fails (C58)", () => {
  assert.deepEqual(k4Of(steeredOver({ reply: false }).turnCompleted("T1").build()), [
    "answered turn T1 has no completed agent message",
  ]);
});

test("k4: only an agent message may be superseded; an uncompleted reasoning item still fails (C58)", () => {
  const wire = new Wire()
    .start("c1")
    .turnStarted("T1")
    .userItem("T1", "u1", "c1")
    .item("started", "T1", "r0", "reasoning")
    .steer("c2", "T1")
    .userItem("T1", "u2", "c2")
    .agentItem("T1", "after")
    .usage("T1", 1234, 200_000)
    .turnCompleted("T1");
  assert.deepEqual(k4Of(wire.build()), ["item r0: completed 0 times"]);
});
