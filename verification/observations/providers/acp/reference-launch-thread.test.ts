import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { evaluateRun, loadCompareRun, RUN_RECORD_FILE } from "./codex-compare-evaluation.ts";
import type { CompareRunRecord } from "./codex-compare-evaluation.ts";
import { SCRIPTED, scriptRun } from "./codex-compare-fixtures.ts";
import { selectLaunchThread, TITLE_THREAD_SOURCE } from "./reference-launch-thread.ts";
import { reduceTurnSkeleton } from "./turn-skeleton.ts";
import type { WireFrame } from "./turn-skeleton.ts";

// The script's thread, as the fixtures name it, and Codex's own title thread.
const LAUNCH = "t";
const TITLE = "019a0000-0000-7000-8000-00000000717e";

/** The launch: the CLI's own first thread/start, answered with the script's thread. */
const launchStart: unknown[] = [
  { id: "start-1", method: "thread/start", params: { threadSource: "user" } },
  { id: "start-1", result: { thread: { id: LAUNCH } } },
];

/**
 * Codex's title flow after the first turn, in the shape of the D0 r1 c32 wire (seq 137-153): a
 * thread/start declaring threadSource thread_title, its reply, thread/started, one title turn
 * with its reply, turn/started, a turn error and turn/completed, then thread/unsubscribe.
 */
function titleFlow(
  source: unknown = TITLE_THREAD_SOURCE,
  turnThread: string = TITLE,
  replyThread: string = TITLE,
): unknown[] {
  return [
    { id: "temp-1", method: "thread/start", params: { threadSource: source, ephemeral: true } },
    { id: "temp-1", result: { thread: { id: replyThread } } },
    { method: "thread/started", params: { thread: { id: TITLE, ephemeral: true } } },
    {
      id: "temp-2",
      method: "turn/start",
      params: { threadId: turnThread, clientUserMessageId: null },
    },
    { id: "temp-2", result: { turn: { id: "title-turn", status: "inProgress" } } },
    { method: "turn/started", params: { threadId: turnThread, turn: { id: "title-turn" } } },
    {
      method: "error",
      params: {
        error: { codexErrorInfo: "usageLimitExceeded" },
        willRetry: false,
        threadId: turnThread,
        turnId: "title-turn",
      },
    },
    {
      method: "turn/completed",
      params: { threadId: turnThread, turn: { id: "title-turn", status: "failed" } },
    },
    { id: "temp-3", method: "thread/unsubscribe", params: { threadId: TITLE } },
  ];
}

/** Wire frames in order; cliToServer for requests (an id and a method), serverToCli otherwise. */
function frames(messages: readonly unknown[], after: readonly WireFrame[] = []): WireFrame[] {
  const start = after.length === 0 ? 0 : Math.max(...after.map((frame) => frame.seq)) + 1;
  return messages.map((message, index) => {
    const isRequest =
      typeof message === "object" && message !== null && "method" in message && "id" in message;
    return {
      seq: start + index,
      connection: 0,
      direction: isRequest ? "cliToServer" : "serverToCli",
      message,
    };
  });
}

function referenceWire(extra: readonly unknown[] = []): WireFrame[] {
  const head = frames(launchStart);
  const script = scriptRun({ turnId: "R1", window: 258_000 })
    .build()
    .map((frame, index) => ({ ...frame, seq: head.length + index }));
  const body = [...head, ...script];
  return [...body, ...frames(extra, body)];
}

test("Codex's title thread is set aside and the judged skeleton is unchanged", () => {
  const plain = referenceWire();
  const withTitle = referenceWire(titleFlow());
  const selection = selectLaunchThread(withTitle);
  assert.equal(selection.launchThreadId, LAUNCH);
  assert.deepEqual(selection.violations, []);
  assert.deepEqual(reduceTurnSkeleton(selection.frames), reduceTurnSkeleton(plain));
  // Without the selection, the title turn would be judged as a second turn of the run.
  assert.notDeepEqual(reduceTurnSkeleton(withTitle).turns, reduceTurnSkeleton(plain).turns);
});

test("negative: an extra thread of any other source fails", () => {
  for (const source of ["user", null, "thread_title_v2", { feature: TITLE_THREAD_SOURCE }]) {
    const selection = selectLaunchThread(referenceWire(titleFlow(source)));
    const label = `source ${String(JSON.stringify(source))}`;
    assert.match(selection.violations[0] ?? "", /an extra thread/u, label);
    // Its frames name a thread that is neither the launch nor a declared title thread (F11).
    assert.ok(
      selection.violations
        .slice(1)
        .every((violation) => violation.includes(`names thread ${TITLE}`)),
      label,
    );
  }
});

test("negative: a title thread whose turn goes into the launch thread fails", () => {
  const selection = selectLaunchThread(referenceWire(titleFlow(TITLE_THREAD_SOURCE, LAUNCH)));
  // A turn into the launch thread without the CLI's client message id is Codex-internal (C48).
  assert.deepEqual(selection.violations, [
    `turn/start at ${referenceWire().length + 3} into the launch thread carries no client message id: a Codex-internal turn, not a scripted input`,
  ]);
  // That turn stays in the judged recording.
  assert.ok(
    reduceTurnSkeleton(selection.frames).turns.some((turn) => turn.turnId === "title-turn"),
  );
});

test("a reference run is judged on its launch thread; an extra thread is a thread violation", () => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "ref-launch-")));
  try {
    const directory = join(root, "run");
    mkdirSync(directory);
    const record: CompareRunRecord = {
      run_kind: "codex-reference",
      wire_recording: "wire.jsonl",
      scripted_settings: SCRIPTED,
      agent_approval_outcomes: null,
    };
    writeFileSync(join(directory, RUN_RECORD_FILE), JSON.stringify(record));
    const write = (wire: readonly WireFrame[]): void => {
      writeFileSync(
        join(directory, "wire.jsonl"),
        wire.map((frame) => JSON.stringify(frame)).join("\n"),
      );
    };
    write(referenceWire(titleFlow()));
    assert.deepEqual(evaluateRun(loadCompareRun(directory)).violations, []);
    write(referenceWire(titleFlow("user")));
    const extra = evaluateRun(loadCompareRun(directory)).violations;
    assert.equal(extra[0]?.rule, "thread");
    assert.match(extra[0]?.detail ?? "", /an extra thread/u);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

/** One reference run directory, judged end to end through loadCompareRun and evaluateRun. */
function judgeReference(wire: readonly WireFrame[]): ReturnType<typeof evaluateRun> & {
  readonly turns: readonly string[];
} {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "ref-judge-")));
  try {
    const directory = join(root, "run");
    mkdirSync(directory);
    const record: CompareRunRecord = {
      run_kind: "codex-reference",
      wire_recording: "wire.jsonl",
      scripted_settings: SCRIPTED,
      agent_approval_outcomes: null,
    };
    writeFileSync(join(directory, RUN_RECORD_FILE), JSON.stringify(record));
    writeFileSync(
      join(directory, "wire.jsonl"),
      wire.map((frame) => JSON.stringify(frame)).join("\n"),
    );
    const loaded = loadCompareRun(directory);
    return { ...evaluateRun(loaded), turns: loaded.skeleton.turns.map((turn) => turn.turnId) };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test("negative: the scripted turns retargeted to an undeclared thread fail, with no second start (F11)", () => {
  const retargeted = referenceWire().map((frame) => ({
    ...frame,
    message: JSON.parse(
      JSON.stringify(frame.message).replaceAll('"threadId":"t"', '"threadId":"X"'),
    ) as unknown,
  }));
  const judged = judgeReference(retargeted);
  const thread = judged.violations.filter((violation) => violation.rule === "thread");
  assert.ok(thread.length > 0);
  assert.ok(
    thread.every((violation) =>
      violation.detail.includes("names thread X, not the launch thread t"),
    ),
  );
  // The same run on its launch thread passes.
  assert.deepEqual(judgeReference(referenceWire()).violations, []);
});

test("negative: a title start answered with the launch id is refused and never erases the launch (F12)", () => {
  const judged = judgeReference(referenceWire(titleFlow(TITLE_THREAD_SOURCE, LAUNCH, LAUNCH)));
  assert.ok(
    judged.violations.some(
      (violation) =>
        violation.rule === "thread" &&
        violation.detail.includes("which is already the launch thread"),
    ),
  );
  // The launch turn is still judged: the collision filtered nothing out.
  assert.ok(judged.turns.includes("R1"));
});

/**
 * The D0 r1 c46 shape (Correction 48), synthetic: before the first prompt the CLI reads two other
 * threads (thread/read, thread/turns/list); after the first turn Codex starts its title thread,
 * the launch's own turn follows with its client id, then the title turn goes into the title thread.
 */
function c46Wire(extra: readonly unknown[] = []): WireFrame[] {
  const reads: unknown[] = [
    { id: "r1", method: "thread/read", params: { threadId: "other-1" } },
    { id: "r2", method: "thread/turns/list", params: { threadId: "other-2" } },
    { id: "r1", result: { thread: { id: "other-1", preview: "owner session body" } } },
    { id: "r2", result: { data: [{ id: "owner-turn" }] } },
  ];
  const head = frames([...launchStart, ...reads]);
  const script = scriptRun({ turnId: "R1", window: 258_000 })
    .build()
    .map((frame, index) => ({ ...frame, seq: head.length + index }));
  const interleaved: unknown[] = [
    { id: "temp-1", method: "thread/start", params: { threadSource: TITLE_THREAD_SOURCE } },
    { id: "u-2", method: "turn/start", params: { threadId: LAUNCH, clientUserMessageId: "c9" } },
    { id: "temp-1", result: { thread: { id: TITLE } } },
    { method: "thread/started", params: { thread: { id: TITLE } } },
    { id: "temp-2", method: "turn/start", params: { threadId: TITLE, clientUserMessageId: null } },
    ...extra,
  ];
  const body = [...head, ...script];
  return [...body, ...frames(interleaved, body)];
}

test("the c46 shape passes: other-thread reads are set aside, the title turn is the title's (C48)", () => {
  const selection = selectLaunchThread(c46Wire());
  assert.deepEqual(selection.violations, []);
  assert.deepEqual(
    selection.setAsideReads.map((read) => [read.method, read.threadHash.length]),
    [
      ["thread/read", 16],
      ["thread/turns/list", 16],
    ],
  );
  // Derived evidence never carries the read thread id or its reply body.
  assert.ok(!JSON.stringify(selection.setAsideReads).includes("other-1"));
  assert.ok(!JSON.stringify(selection.frames).includes("owner session body"));
});

test("negative: a turn, a steer or another method naming another thread still fails (C48)", () => {
  for (const request of [
    { id: "x1", method: "turn/start", params: { threadId: "other-1", clientUserMessageId: "c5" } },
    { id: "x2", method: "turn/steer", params: { threadId: "other-1", clientUserMessageId: "c6" } },
    { id: "x3", method: "thread/resume", params: { threadId: "other-1" } },
  ]) {
    const selection = selectLaunchThread(c46Wire([request]));
    assert.ok(
      selection.violations.some((violation) => violation.includes("names thread other-1")),
      request.method,
    );
  }
});

test("negative: a launch-thread turn without a client id, or the title turn sent into LAUNCH, fails (C48)", () => {
  const internal = selectLaunchThread(
    c46Wire([
      { id: "x4", method: "turn/start", params: { threadId: LAUNCH, clientUserMessageId: null } },
    ]),
  );
  assert.ok(
    internal.violations.some((violation) => violation.includes("carries no client message id")),
  );
  const intoLaunch = selectLaunchThread(referenceWire(titleFlow(TITLE_THREAD_SOURCE, LAUNCH)));
  assert.ok(
    intoLaunch.violations.some((violation) => violation.includes("carries no client message id")),
  );
});

test("a server request or CLI reply sharing a set-aside read's id stays judged (C49)", () => {
  // JSON-RPC ids are numbered separately on each side: the server's approval request may reuse
  // "r1", the id of the CLI's set-aside thread/read, on the same connection.
  const approval = {
    id: "r1",
    method: "item/commandExecution/requestApproval",
    params: { threadId: LAUNCH, turnId: "R1", itemId: "approval-item" },
  };
  const answer = { id: "r1", result: { decision: "accept" } };
  const wire = c46Wire();
  const start = Math.max(...wire.map((frame) => frame.seq)) + 1;
  const extra: WireFrame[] = [
    { seq: start, connection: 0, direction: "serverToCli", message: approval },
    { seq: start + 1, connection: 0, direction: "cliToServer", message: answer },
  ];
  const selection = selectLaunchThread([...wire, ...extra]);
  assert.ok(
    selection.frames.some((frame) => frame.message === approval),
    "approval request dropped",
  );
  assert.ok(
    selection.frames.some((frame) => frame.message === answer),
    "CLI reply dropped",
  );
  // The read and its own reply are still set aside.
  assert.equal(selection.setAsideReads.length, 2);
});

test("a server request sharing a title thread request's id stays judged (C49)", () => {
  const approval = {
    id: "temp-1",
    method: "item/commandExecution/requestApproval",
    params: { threadId: LAUNCH, turnId: "R1", itemId: "approval-item" },
  };
  const answer = { id: "temp-1", result: { decision: "accept" } };
  const wire = referenceWire(titleFlow());
  const next = Math.max(...wire.map((frame) => frame.seq)) + 1;
  const selection = selectLaunchThread([
    ...wire,
    { seq: next, connection: 0, direction: "serverToCli", message: approval },
    { seq: next + 1, connection: 0, direction: "cliToServer", message: answer },
  ]);
  assert.ok(
    selection.frames.some((frame) => frame.message === approval),
    "approval request dropped",
  );
  assert.ok(
    selection.frames.some((frame) => frame.message === answer),
    "CLI reply dropped",
  );
});

test("negative: a later request reusing a settled title request's id is judged, and fails F11 (F15)", () => {
  // The title flow's start (id "temp-1") is answered, so the id is free; the CLI then reuses it
  // for a turn/start to another thread.
  const wire = referenceWire(titleFlow());
  const next = Math.max(...wire.map((frame) => frame.seq)) + 1;
  const reused = {
    id: "temp-1",
    method: "turn/start",
    params: { threadId: "other-thread", clientUserMessageId: "c7" },
  };
  const selection = selectLaunchThread([
    ...wire,
    { seq: next, connection: 0, direction: "cliToServer", message: reused },
  ]);
  assert.ok(
    selection.frames.some((frame) => frame.message === reused),
    "reused-id request dropped",
  );
  assert.ok(
    selection.violations.some((violation) => violation.includes("names thread other-thread")),
  );
});

test("a launch turn reusing a settled history read's id, and its reply, stay judged (F15)", () => {
  const wire = c46Wire();
  const next = Math.max(...wire.map((frame) => frame.seq)) + 1;
  const turn = {
    id: "r1",
    method: "turn/start",
    params: { threadId: LAUNCH, clientUserMessageId: "c8" },
  };
  const reply = { id: "r1", result: { turn: { id: "R2", status: "inProgress" } } };
  const selection = selectLaunchThread([
    ...wire,
    { seq: next, connection: 0, direction: "cliToServer", message: turn },
    { seq: next + 1, connection: 0, direction: "serverToCli", message: reply },
  ]);
  assert.ok(
    selection.frames.some((frame) => frame.message === turn),
    "reused-id turn dropped",
  );
  assert.ok(
    selection.frames.some((frame) => frame.message === reply),
    "its reply dropped",
  );
  assert.equal(selection.setAsideReads.length, 2);
  assert.deepEqual(selection.violations, []);
});

test("a reply that settles no outstanding request is judged, even with a settled read's id (F15)", () => {
  const wire = c46Wire();
  const next = Math.max(...wire.map((frame) => frame.seq)) + 1;
  const stray = { id: "r1", result: { thread: { id: "other-1" } } };
  const selection = selectLaunchThread([
    ...wire,
    { seq: next, connection: 0, direction: "serverToCli", message: stray },
  ]);
  assert.ok(
    selection.frames.some((frame) => frame.message === stray),
    "stray reply set aside",
  );
});

test('numeric 7 and string "7" are different requests, both outstanding at once (F16)', () => {
  const wire = referenceWire();
  const next = Math.max(...wire.map((frame) => frame.seq)) + 1;
  const read = { id: 7, method: "thread/read", params: { threadId: "other-1" } };
  const turn = {
    id: "7",
    method: "turn/start",
    params: { threadId: LAUNCH, clientUserMessageId: "c7" },
  };
  const turnReply = { id: "7", result: { turn: { id: "R7", status: "inProgress" } } };
  const readReply = { id: 7, result: { thread: { id: "other-1" } } };
  const selection = selectLaunchThread([
    ...wire,
    { seq: next, connection: 0, direction: "cliToServer", message: read },
    { seq: next + 1, connection: 0, direction: "cliToServer", message: turn },
    { seq: next + 2, connection: 0, direction: "serverToCli", message: turnReply },
    { seq: next + 3, connection: 0, direction: "serverToCli", message: readReply },
  ]);
  const judged = (message: unknown): boolean =>
    selection.frames.some((frame) => frame.message === message);
  assert.ok(!judged(read) && !judged(readReply), "the read and its reply are set aside");
  assert.ok(judged(turn) && judged(turnReply), "the turn and its reply are judged");
  assert.equal(selection.setAsideReads.length, 1);
  assert.deepEqual(selection.violations, []);
});
