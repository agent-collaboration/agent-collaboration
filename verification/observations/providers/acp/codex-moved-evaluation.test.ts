import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  checkResumedThread,
  checkTierOnWire,
  evaluateMovedRun,
  loadMovedRun,
  MOVED_RUN_FILE,
} from "./codex-moved-evaluation.ts";
import { LAUNCH_THREAD, OTHER_THREAD, resumeRun, tierRun } from "./codex-moved-fixtures.ts";
import { reduceTurnSkeleton } from "./turn-skeleton.ts";
import type { WireFrame } from "./turn-skeleton.ts";

const EXPECTATION = { launchThreadId: LAUNCH_THREAD, expectedThreadId: OTHER_THREAD };

test("resume: the pick resumes exactly the declared other thread", () => {
  assert.deepEqual(checkResumedThread(resumeRun(), EXPECTATION), []);
});

test("negative: /resume resumed another thread", () => {
  const wrong = checkResumedThread(resumeRun({ pickedThread: "someone-else" }), EXPECTATION);
  assert.ok(wrong.some((violation) => violation.detail.includes("not " + OTHER_THREAD)));
  assert.ok(wrong.some((violation) => violation.detail.includes(`no thread/resume for`)));
});

test("negative: the pick never resumed, was refused, or started a new thread", () => {
  assert.ok(checkResumedThread(resumeRun({ skipPick: true }), EXPECTATION).length > 0);
  assert.deepEqual(
    checkResumedThread(resumeRun({ refusePick: true }), EXPECTATION).map(
      (violation) => violation.detail,
    ),
    [`resume of ${OTHER_THREAD} was refused`],
  );
  assert.ok(
    checkResumedThread(resumeRun({ startNewThreadInstead: true }), EXPECTATION).some((violation) =>
      violation.detail.includes("a new thread"),
    ),
  );
});

test("negative: resuming the launch thread again cannot be told from the pick", () => {
  assert.ok(
    checkResumedThread(resumeRun(), {
      launchThreadId: OTHER_THREAD,
      expectedThreadId: OTHER_THREAD,
    }).length > 0,
  );
});

test("resume, real-Codex reference: its own launch start is the launch, the pick is judged (F3)", () => {
  assert.deepEqual(checkResumedThread(resumeRun({ launch: "start" }), EXPECTATION), []);
  // A second start after that launch is still a new thread.
  assert.ok(
    checkResumedThread(
      resumeRun({ launch: "start", startNewThreadInstead: true }),
      EXPECTATION,
    ).some((violation) => violation.detail.includes("thread/start at 2: a new thread")),
  );
  // A start answered with another thread is not the declared launch.
  assert.ok(
    checkResumedThread(resumeRun({ launch: "start" }), {
      launchThreadId: "not-the-launch",
      expectedThreadId: OTHER_THREAD,
    }).some((violation) => violation.detail.includes("thread/start at 0: a new thread")),
  );
});

test("negative: the pick has no reply, an empty one, or one for another thread (F7)", () => {
  for (const launch of ["resume", "start"] as const) {
    assert.deepEqual(
      checkResumedThread(resumeRun({ launch, pickAnswer: "none" }), EXPECTATION).map(
        (violation) => violation.detail,
      ),
      [`resume of ${OTHER_THREAD} at 2 was never answered`],
    );
    assert.deepEqual(
      checkResumedThread(resumeRun({ launch, pickAnswer: "empty" }), EXPECTATION).map(
        (violation) => violation.detail,
      ),
      [`resume of ${OTHER_THREAD} at 2 answered with thread undefined, not ${OTHER_THREAD}`],
    );
    assert.deepEqual(
      checkResumedThread(resumeRun({ launch, pickAnswer: "other" }), EXPECTATION).map(
        (violation) => violation.detail,
      ),
      [`resume of ${OTHER_THREAD} at 2 answered with thread someone-else, not ${OTHER_THREAD}`],
    );
  }
});

test("tier: the last settings carry the picked tier", () => {
  const skeleton = (tier: string | null) => reduceTurnSkeleton(tierRun({ tier }));
  assert.deepEqual(checkTierOnWire(skeleton("priority"), "priority"), []);
});

test("negative: the tier is unchanged or cleared on the wire, or no settings arrived", () => {
  const skeleton = reduceTurnSkeleton(tierRun({ tier: null }));
  assert.equal(checkTierOnWire(skeleton, "priority").length, 1);
  assert.equal(
    checkTierOnWire(reduceTurnSkeleton(tierRun({ tier: "flex" })), "priority").length,
    1,
  );
  assert.equal(
    checkTierOnWire(reduceTurnSkeleton(tierRun({ tier: null, omitSettings: true })), "priority")
      .length,
    1,
  );
});

function runDirectory(
  record: unknown,
  frames: readonly WireFrame[],
): { directory: string; cleanup: () => void } {
  const directory = realpathSync(mkdtempSync(join(tmpdir(), "moved-run-")));
  mkdirSync(directory, { recursive: true });
  writeFileSync(
    join(directory, "wire.jsonl"),
    frames.map((frame) => JSON.stringify(frame)).join("\n"),
  );
  writeFileSync(join(directory, MOVED_RUN_FILE), JSON.stringify(record));
  return { directory, cleanup: () => rmSync(directory, { recursive: true, force: true }) };
}

test("a run directory is loaded and judged end to end", () => {
  const resume = runDirectory(
    {
      kind: "resume",
      wire_recording: "wire.jsonl",
      launch_thread_id: LAUNCH_THREAD,
      expected_thread_id: OTHER_THREAD,
    },
    resumeRun(),
  );
  const tier = runDirectory(
    { kind: "tier", wire_recording: "wire.jsonl", wire_value: "priority" },
    tierRun({ tier: null }),
  );
  try {
    assert.deepEqual(evaluateMovedRun(loadMovedRun(resume.directory)), []);
    assert.equal(evaluateMovedRun(loadMovedRun(tier.directory)).length, 1);
  } finally {
    resume.cleanup();
    tier.cleanup();
  }
});

test("a window merged into another connection's recording is refused", () => {
  const merged = [...resumeRun({ connection: 0 }), ...resumeRun({ connection: 1 })].map(
    (frame, seq) => ({ ...frame, seq }),
  );
  const run = runDirectory(
    {
      kind: "resume",
      wire_recording: "wire.jsonl",
      launch_thread_id: LAUNCH_THREAD,
      expected_thread_id: OTHER_THREAD,
    },
    merged,
  );
  try {
    assert.throws(() => loadMovedRun(run.directory), /exactly one CLI connection/);
  } finally {
    run.cleanup();
  }
});

test("negative: a tier kept by an update that omits it is not a clear (F6)", () => {
  // The review's counterexample: on priority, the next update omits the tier, so priority is
  // retained; a script expecting the tier cleared must not pass.
  const frames: WireFrame[] = [
    {
      seq: 0,
      connection: 0,
      direction: "serverToCli",
      message: {
        method: "thread/settings/updated",
        params: {
          threadId: "t",
          threadSettings: { model: "gpt-x", effort: "low", serviceTier: "priority" },
        },
      },
    },
    {
      seq: 1,
      connection: 0,
      direction: "serverToCli",
      message: {
        method: "thread/settings/updated",
        params: { threadId: "t", threadSettings: { model: "gpt-x", effort: "low" } },
      },
    },
  ];
  const skeleton = reduceTurnSkeleton(frames);
  assert.equal(skeleton.lastSettings?.serviceTier, "priority");
  assert.deepEqual(checkTierOnWire(skeleton, "priority"), []);
});
