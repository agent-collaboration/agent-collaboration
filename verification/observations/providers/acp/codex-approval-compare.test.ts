import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  ACP_OPTIONS_WITHOUT_CODEX_EQUIVALENT,
  APPROVAL_RUN_FILE,
  CODEX_DECISION_TO_ACP,
  compareApprovalRuns,
  evaluateApprovalRun,
  loadApprovalRun,
} from "./codex-approval-compare.ts";
import type { ApprovalRunRecord, LoadedApprovalRun } from "./codex-approval-compare.ts";
import { APPROVAL_OUTCOME_FILE, approvalOutcomeFile } from "./codex-approval-outcome.ts";
import type { ApprovalOutcomeFile, ApprovalRecord } from "./codex-approval-outcome.ts";
import { approvalPairRun } from "./codex-approval-fixtures.ts";
import type { ApprovalPairOptions } from "./codex-approval-fixtures.ts";

const base = (kind: ApprovalRunRecord["run_kind"], choice: string | null): ApprovalRunRecord => ({
  run_kind: kind,
  wire_recording: "wire.jsonl",
  window_a: 0,
  window_b: 1,
  action_kind: "command",
  cli_choice: choice,
});

const record = (state: string): ApprovalRecord => ({
  requestId: "r1",
  state,
  options: [
    { optionId: "a", effect: "allow", scope: "once", persistentTarget: null },
    { optionId: "s", effect: "allow", scope: "session", persistentTarget: null },
    { optionId: "d", effect: "decline", scope: "once", persistentTarget: null },
  ],
});

function reading(
  name: "allow-once" | "allow-session" | "decline" | "timeout",
  decision: string | null,
  state: string,
  filePresent: boolean,
): ApprovalOutcomeFile {
  return approvalOutcomeFile({
    case: name,
    cliDecision: decision,
    readings: { pending: record("pendingClientDecision"), final: record(state), filePresent },
  });
}

const reference = (
  choice: string | null = "accept",
  wire: ApprovalPairOptions = {},
): LoadedApprovalRun => ({
  record: base("codex-reference", choice),
  frames: approvalPairRun({
    decision: choice,
    itemStatus: choice === "decline" ? "declined" : "completed",
    ...wire,
  }),
  agentReading: null,
});
const candidate = (
  choice: string | null,
  agentReading: ApprovalOutcomeFile,
  wire: ApprovalPairOptions = {},
): LoadedApprovalRun => ({
  record: base("router-candidate", choice),
  frames: approvalPairRun({ decision: choice, ...wire }),
  agentReading,
});
const allowed = (): LoadedApprovalRun =>
  candidate("accept", reading("allow-once", "accept", "decided", true));

test("the table maps each Codex choice to its ACP option, and names what has no equivalent", () => {
  assert.deepEqual(CODEX_DECISION_TO_ACP, {
    accept: { acpOptionKind: "allow_once", outcome: "allow" },
    acceptForSession: { acpOptionKind: "allow_always", outcome: "allow" },
    decline: { acpOptionKind: "reject_once", outcome: "reject" },
  });
  assert.equal(ACP_OPTIONS_WITHOUT_CODEX_EQUIVALENT.length, 3);
});

test("matching runs pass for allow once, allow for the session and decline", () => {
  assert.equal(compareApprovalRuns(reference(), allowed()).status, "pass");
  assert.equal(
    compareApprovalRuns(
      reference("acceptForSession"),
      candidate("acceptForSession", reading("allow-session", "acceptForSession", "decided", true)),
    ).status,
    "pass",
  );
  assert.equal(
    compareApprovalRuns(
      reference("decline"),
      candidate("decline", reading("decline", "decline", "decided", false)),
    ).status,
    "pass",
  );
});

test("a timeout on the candidate gives its declared outcome", () => {
  const timeout = candidate(null, reading("timeout", null, "timedOut", false));
  assert.deepEqual(evaluateApprovalRun(timeout).violations, []);
  const decided = candidate(null, reading("timeout", null, "decided", false));
  assert.ok(evaluateApprovalRun(decided).violations.some((v) => v.rule === "a2"));
});

test("negative a1: a different request method, a missing choice, a prompt only in text", () => {
  const method = compareApprovalRuns(
    reference(),
    candidate("accept", reading("allow-once", "accept", "decided", true), {
      method: "item/fileChange/requestApproval",
    }),
  );
  assert.equal(method.status, "fail");
  assert.ok(method.candidate.some((v) => v.rule === "a1"));
  assert.ok(method.crossRun.some((v) => v.detail.includes("request method")));
  const missing = compareApprovalRuns(
    reference(),
    candidate("accept", reading("allow-once", "accept", "decided", true), {
      choices: ["accept", "decline", "cancel"],
    }),
  );
  assert.ok(missing.crossRun.some((v) => v.detail.includes("native choices")));
  const text = compareApprovalRuns(
    reference(),
    candidate("accept", reading("allow-once", "accept", "decided", true), { textOnly: true }),
  );
  assert.ok(text.candidate.some((v) => v.detail.includes("can only have been text")));
});

test("negative a2: a wrong agent outcome, or none", () => {
  const wrong = compareApprovalRuns(
    reference(),
    candidate("accept", reading("allow-once", "accept", "decided", false)),
  );
  assert.ok(wrong.candidate.some((v) => v.rule === "a2" && v.detail.includes("allow_once")));
  const declinedForAllow = compareApprovalRuns(
    reference(),
    candidate("accept", reading("decline", "decline", "decided", false)),
  );
  assert.ok(declinedForAllow.candidate.some((v) => v.rule === "a2"));
  const none = compareApprovalRuns(
    reference(),
    candidate("accept", reading("allow-once", "accept", "pendingClientDecision", true)),
  );
  assert.ok(none.candidate.some((v) => v.rule === "a2" && v.detail.includes("none")));
});

test("negative a2: a choice outside the table, and the CLI sending another choice than scripted", () => {
  const cancelled = evaluateApprovalRun({
    ...reference("cancel"),
    frames: approvalPairRun({ decision: "cancel", itemStatus: "declined" }),
  });
  assert.ok(cancelled.violations.some((v) => v.detail.includes("no ACP outcome in the N7 table")));
  const other = compareApprovalRuns(
    reference(),
    candidate("accept", reading("allow-once", "accept", "decided", true), { decision: "decline" }),
  );
  assert.ok(other.candidate.some((v) => v.detail.includes("script chose accept")));
});

test("negative a3: both windows prompted, and the result never shown in the second window", () => {
  const both = compareApprovalRuns(
    reference(),
    candidate("accept", reading("allow-once", "accept", "decided", true), { bPrompted: true }),
  );
  assert.ok(
    both.candidate.some(
      (v) => v.rule === "a3" && v.detail.includes("watching window was prompted"),
    ),
  );
  const unseen = compareApprovalRuns(
    reference(),
    candidate("accept", reading("allow-once", "accept", "decided", true), { bMissesResult: true }),
  );
  assert.ok(unseen.candidate.some((v) => v.detail.includes("result never reached")));
  const alone = compareApprovalRuns(
    reference(),
    candidate("accept", reading("allow-once", "accept", "decided", true), { noWindowB: true }),
  );
  assert.ok(alone.candidate.some((v) => v.detail.includes("second window never connected")));
});

test("negative a3: the watching window must see the approval itself, not another item (review F5)", () => {
  const ok = reading("allow-once", "accept", "decided", true);
  // An unrelated agent message starts before the answer and completes after it, while the
  // approval's own pending item and result never reach B.
  const substituted = compareApprovalRuns(
    reference(),
    candidate("accept", ok, { bSubstitutes: true }),
  );
  assert.equal(substituted.status, "fail");
  assert.ok(
    substituted.candidate.some(
      (v) => v.rule === "a3" && v.detail.includes("never saw the approval item item-1"),
    ),
  );
  assert.ok(
    substituted.candidate.some((v) => v.rule === "a3" && v.detail.includes("result never reached")),
  );
  const otherTurn = compareApprovalRuns(reference(), candidate("accept", ok, { bTurnId: "T2" }));
  assert.ok(otherTurn.candidate.some((v) => v.rule === "a3" && v.detail.includes("turn T1")));
  const otherResult = compareApprovalRuns(
    reference(),
    candidate("accept", ok, { bResultStatus: "declined" }),
  );
  assert.ok(
    otherResult.candidate.some(
      (v) =>
        v.rule === "a3" && v.detail.includes("saw result declined, the approving window completed"),
    ),
  );
});

test("negative a3: a timed-out prompt still owes the watching window the approval's result", () => {
  const timedOut = reading("timeout", null, "timedOut", false);
  assert.deepEqual(
    evaluateApprovalRun(candidate(null, timedOut, { itemStatus: "declined" })).violations,
    [],
  );
  const missed = evaluateApprovalRun(
    candidate(null, timedOut, { itemStatus: "declined", bMissesResult: true }),
  );
  assert.ok(
    missed.violations.some((v) => v.rule === "a3" && v.detail.includes("result never reached")),
  );
  const substituted = evaluateApprovalRun(
    candidate(null, timedOut, { itemStatus: "declined", bSubstitutes: true }),
  );
  assert.ok(substituted.violations.some((v) => v.rule === "a3" && v.detail.includes("pending")));
});

test("a rule real Codex breaks is reported against the reference and never relaxed", () => {
  const verdict = compareApprovalRuns(
    { ...reference(), frames: approvalPairRun({ bPrompted: true }) },
    allowed(),
  );
  assert.equal(verdict.status, "rule-contradicted-by-reference");
});

test("run directories load; a candidate without its agent-side reading is refused", () => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "apr-cmp-")));
  try {
    const dir = (name: string, run: LoadedApprovalRun, outcome: boolean): string => {
      const directory = join(root, name);
      mkdirSync(directory);
      writeFileSync(
        join(directory, "wire.jsonl"),
        run.frames.map((frame) => JSON.stringify(frame)).join("\n"),
      );
      writeFileSync(join(directory, APPROVAL_RUN_FILE), JSON.stringify(run.record));
      if (outcome && run.agentReading !== null)
        writeFileSync(join(directory, APPROVAL_OUTCOME_FILE), JSON.stringify(run.agentReading));
      return directory;
    };
    const loaded = compareApprovalRuns(
      loadApprovalRun(dir("ref", reference(), false)),
      loadApprovalRun(dir("cand", allowed(), true)),
    );
    assert.equal(loaded.status, "pass");
    assert.throws(() => loadApprovalRun(dir("bare", allowed(), false)), /agent side was not read/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a real-Codex reference's scratch file must agree with its own outcome", () => {
  const agrees = { ...reference(), scratchFilePresent: true };
  assert.deepEqual(evaluateApprovalRun(agrees).violations, []);
  const disagrees = { ...reference(), scratchFilePresent: false };
  assert.ok(
    evaluateApprovalRun(disagrees).violations.some((v) => v.detail.includes("scratch file")),
  );
});
