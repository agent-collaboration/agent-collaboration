import assert from "node:assert/strict";
import { test } from "node:test";

import { checkTwoWindows } from "./codex-window-evaluation.ts";
import type { WireFrame } from "./turn-skeleton.ts";

const WINDOWS = { windowA: 0, windowB: 1 } as const;

interface Options {
  /** B never gets A's turn. */
  readonly bMissesTurn?: boolean;
  /** B receives A's user item twice. */
  readonly bDuplicateReceipt?: boolean;
  readonly bStatus?: string;
  readonly bStartsTurn?: boolean;
  readonly noWindowB?: boolean;
  readonly bMissesAgentItem?: boolean;
  /** B's frames carry connection 0, merged into A's. */
  readonly bMergedIntoA?: boolean;
  /** B attaches only after A's judged turn has ended. */
  readonly bAttachesLate?: boolean;
}

function windowFrames(
  connection: number,
  input: {
    turnId: string;
    clientId: string;
    status: string;
    receipts: number;
    agent: boolean;
    startsTurn: boolean;
  },
): unknown[][] {
  const turn = (phase: string, status: string): [string, unknown] => [
    "serverToCli",
    { method: `turn/${phase}`, params: { threadId: "t", turn: { id: input.turnId, status } } },
  ];
  const item = (phase: string, id: string, type: string, clientId?: string): [string, unknown] => [
    "serverToCli",
    {
      method: `item/${phase}`,
      params: {
        threadId: "t",
        turnId: input.turnId,
        item: { id, type, ...(clientId === undefined ? {} : { clientId }) },
      },
    },
  ];
  const rows: [string, unknown][] = [];
  if (input.startsTurn)
    rows.push([
      "cliToServer",
      { id: 1, method: "turn/start", params: { clientUserMessageId: input.clientId } },
    ]);
  rows.push(turn("started", "inProgress"));
  for (let index = 0; index < input.receipts; index += 1) {
    rows.push(item("started", `u${index}`, "userMessage", input.clientId));
    rows.push(item("completed", `u${index}`, "userMessage", input.clientId));
  }
  if (input.agent) {
    rows.push(item("started", "a1", "agentMessage"));
    rows.push([
      "serverToCli",
      { method: "item/agentMessage/delta", params: { turnId: input.turnId, itemId: "a1" } },
    ]);
    rows.push(item("completed", "a1", "agentMessage"));
  }
  rows.push(turn("completed", input.status));
  return rows.map(([direction, message]) => [connection, direction, message]);
}

/**
 * Correction 32's two-turn script: A's turn T0 materializes the thread before B attaches, B
 * connects (its thread/resume), then A's judged turn T1, which B mirrors.
 */
function recording(options: Options = {}): readonly WireFrame[] {
  const aTurn = (turnId: string, clientId: string): unknown[][] =>
    windowFrames(0, {
      turnId,
      clientId,
      status: "completed",
      receipts: 1,
      agent: true,
      startsTurn: true,
    });
  const bConnection = options.bMergedIntoA === true ? 0 : 1;
  const attach: unknown[][] =
    options.noWindowB === true
      ? []
      : [
          [
            bConnection,
            "cliToServer",
            { id: 9, method: "thread/resume", params: { threadId: "t" } },
          ],
        ];
  const mirror: unknown[][] =
    options.noWindowB === true || options.bMissesTurn === true
      ? []
      : windowFrames(bConnection, {
          turnId: "T1",
          clientId: "c1",
          status: options.bStatus ?? "completed",
          receipts: options.bDuplicateReceipt === true ? 2 : 1,
          agent: options.bMissesAgentItem !== true,
          startsTurn: options.bStartsTurn === true,
        });
  const rows =
    options.bAttachesLate === true
      ? [...aTurn("T0", "c0"), ...aTurn("T1", "c1"), ...attach, ...mirror]
      : [...aTurn("T0", "c0"), ...attach, ...aTurn("T1", "c1"), ...mirror];
  return rows.map(
    ([connection, direction, message], seq) =>
      ({ seq, connection, direction, message }) as WireFrame,
  );
}

const rules = (options?: Options): string[] =>
  checkTwoWindows(recording(options), WINDOWS).map((violation) => violation.rule);

test("B mirrors A's turn, receipt and status, and starts nothing", () => {
  assert.deepEqual(checkTwoWindows(recording(), WINDOWS), []);
});

test("negative: B misses A's turn", () => {
  assert.deepEqual(rules({ bMissesTurn: true }), ["w2", "w3"]);
});

test("negative: a duplicate receipt in B", () => {
  const found = checkTwoWindows(recording({ bDuplicateReceipt: true }), WINDOWS);
  assert.ok(
    found.some((violation) => violation.rule === "w3" && violation.detail.includes("2 times")),
  );
});

test("negative: a different status in B", () => {
  const found = checkTwoWindows(recording({ bStatus: "failed" }), WINDOWS);
  assert.deepEqual(
    found.map((violation) => violation.rule),
    ["w2"],
  );
  assert.match(found[0]?.detail ?? "", /status completed in A, failed in B/);
});

test("negative: the missing second connection", () => {
  assert.deepEqual(checkTwoWindows(recording({ noWindowB: true }), WINDOWS), [
    { rule: "w1", detail: "window B never connected" },
  ]);
});

test("negative: B starting a turn", () => {
  const found = checkTwoWindows(recording({ bStartsTurn: true }), WINDOWS);
  assert.ok(found.some((violation) => violation.rule === "w4"));
});

test("negative: B lacking an item A had, and B's frames merged into A's connection", () => {
  assert.ok(rules({ bMissesAgentItem: true }).includes("w2"));
  assert.deepEqual(checkTwoWindows(recording({ bMergedIntoA: true }), WINDOWS)[0]?.rule, "w1");
});

test("a recording where both windows are one connection is refused", () => {
  assert.equal(checkTwoWindows(recording(), { windowA: 0, windowB: 0 })[0]?.rule, "w1");
});

test("A's turn 1 before B attached is not judged; turn 2 is (Correction 32)", () => {
  // T0 is never mirrored in B and does not count against it.
  assert.deepEqual(checkTwoWindows(recording(), WINDOWS), []);
});

test("negative: B attached only after A's last turn leaves nothing judged", () => {
  assert.deepEqual(
    checkTwoWindows(recording({ bAttachesLate: true }), WINDOWS).filter(
      (violation) => violation.rule === "w2",
    ),
    [
      {
        rule: "w2",
        detail: "window A started no turn after window B attached; nothing was mirrored",
      },
    ],
  );
});
