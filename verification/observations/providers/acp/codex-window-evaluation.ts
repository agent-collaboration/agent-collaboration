/**
 * O5 two windows (Verification Design #O5-windows): window B attaches to the same thread while
 * window A drives the script. The tee tags every frame with its CLI connection, so each window is
 * reduced on its own and these rules are checked:
 *
 *   w1  each window's own frames pass k1-k4;
 *   w2  every turn A starts after B connected appears in B with the same turn id, started before
 *       and completed after the same items, with the same status;
 *   w3  each input A sent after B connected appears in B as exactly one user item with A's client
 *       message id;
 *   w4  B sends no turn/start or turn/steer of its own.
 *
 * Built and unit-tested only; D3 runs it live, real Codex first and then the Router.
 */
import { checkRunRules, reduceTurnSkeleton } from "./turn-skeleton.ts";
import type { SkeletonTurn, TurnSkeleton, WireFrame } from "./turn-skeleton.ts";

export type WindowRule = "w1" | "w2" | "w3" | "w4";

export interface WindowViolation {
  readonly rule: WindowRule;
  readonly detail: string;
}

export interface WindowConnections {
  /** The tee connection of the window that drives the script. */
  readonly windowA: number;
  /** The tee connection of the window that only watches. */
  readonly windowB: number;
}

function itemIds(turn: SkeletonTurn): ReadonlySet<string> {
  return new Set(
    turn.entries.flatMap((entry) => (entry.itemId === undefined ? [] : [entry.itemId])),
  );
}

function userReceipts(skeleton: TurnSkeleton, clientId: string): number {
  return skeleton.turns.reduce(
    (count, turn) =>
      count +
      turn.entries.filter(
        (entry) =>
          entry.kind === "itemStarted" &&
          entry.itemType === "userMessage" &&
          entry.clientId === clientId,
      ).length,
    0,
  );
}

function turnMirrorViolations(
  turn: SkeletonTurn,
  mirror: SkeletonTurn | undefined,
): WindowViolation[] {
  const w2 = (detail: string): WindowViolation => ({
    rule: "w2",
    detail: `turn ${turn.turnId}: ${detail}`,
  });
  if (mirror === undefined) return [w2("window B never saw it")];
  const violations: WindowViolation[] = [];
  if (mirror.status !== turn.status)
    violations.push(w2(`status ${String(turn.status)} in A, ${String(mirror.status)} in B`));
  const started = mirror.startedSeqs[0];
  const completed = mirror.completedSeqs[0];
  if (started === undefined || completed === undefined)
    violations.push(w2("window B lacks its start or its completion"));
  else {
    const outside = mirror.entries.filter((entry) => entry.seq < started || entry.seq > completed);
    if (outside.length > 0)
      violations.push(w2("window B has items outside its start and completion"));
  }
  const expected = itemIds(turn);
  const seen = itemIds(mirror);
  for (const id of expected)
    if (!seen.has(id)) violations.push(w2(`item ${id} missing in window B`));
  for (const id of seen) if (!expected.has(id)) violations.push(w2(`item ${id} only in window B`));
  return violations;
}

/** Splits a two-window recording by connection and reduces each window on its own. */
export function reduceWindows(
  frames: readonly WireFrame[],
  connections: WindowConnections,
): { readonly a: TurnSkeleton; readonly b: TurnSkeleton } | { readonly missing: string } {
  const present = new Set(frames.map((frame) => frame.connection));
  if (!present.has(connections.windowA)) return { missing: "window A has no connection" };
  if (!present.has(connections.windowB)) return { missing: "window B never connected" };
  const only = (connection: number): WireFrame[] =>
    frames.filter((frame) => frame.connection === connection);
  return {
    a: reduceTurnSkeleton(only(connections.windowA)),
    b: reduceTurnSkeleton(only(connections.windowB)),
  };
}

/**
 * Every violation of w1-w4 in one two-window recording; empty means it passes. B attaches after A's
 * first turn materialized the thread (assignment Correction 32), so w2 and w3 judge the turns and
 * inputs A starts after B's first frame; at least one is required. w1 still covers each whole window.
 */
export function checkTwoWindows(
  frames: readonly WireFrame[],
  connections: WindowConnections,
): WindowViolation[] {
  if (connections.windowA === connections.windowB)
    return [{ rule: "w1", detail: "windows A and B are the same connection" }];
  const windows = reduceWindows(frames, connections);
  if ("missing" in windows) return [{ rule: "w1", detail: windows.missing }];
  const { a, b } = windows;
  // The tee numbers frames across connections, so B's first frame marks when B was attached.
  const attachedAt = Math.min(
    ...frames.filter((frame) => frame.connection === connections.windowB).map((frame) => frame.seq),
  );
  const judgedTurns = a.turns.filter((turn) => (turn.startedSeqs[0] ?? -1) > attachedAt);
  const judgedInputs = a.inputs.filter((input) => input.requestSeq > attachedAt);
  const violations: WindowViolation[] = [
    ...checkRunRules(a).map((violation) => ({
      rule: "w1" as const,
      detail: `window A ${violation.rule}: ${violation.detail}`,
    })),
    ...checkRunRules(b).map((violation) => ({
      rule: "w1" as const,
      detail: `window B ${violation.rule}: ${violation.detail}`,
    })),
  ];
  if (judgedTurns.length === 0)
    violations.push({
      rule: "w2",
      detail: "window A started no turn after window B attached; nothing was mirrored",
    });
  for (const turn of judgedTurns)
    violations.push(
      ...turnMirrorViolations(
        turn,
        b.turns.find((other) => other.turnId === turn.turnId),
      ),
    );
  for (const input of judgedInputs) {
    if (input.clientId === undefined) {
      violations.push({
        rule: "w3",
        detail: `A's input ${input.ordinal} sent no client message id`,
      });
      continue;
    }
    const receipts = userReceipts(b, input.clientId);
    if (receipts !== 1)
      violations.push({
        rule: "w3",
        detail: `A's input ${input.ordinal} appears in window B ${receipts} times, not once`,
      });
  }
  for (const input of b.inputs)
    violations.push({
      rule: "w4",
      detail: `window B sent ${input.kind === "start" ? "turn/start" : "turn/steer"} at ${input.requestSeq}`,
    });
  return violations;
}
