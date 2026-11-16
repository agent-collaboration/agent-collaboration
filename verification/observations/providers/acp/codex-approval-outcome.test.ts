import assert from "node:assert/strict";
import { test } from "node:test";

import {
  agentApprovalOutcomesFor,
  approvalOutcomeFile,
  approvalRecordsFrom,
  checkApprovalAgainstWire,
  evaluateApprovalCase,
} from "./codex-approval-outcome.ts";
import type { ApprovalCase, ApprovalReadings } from "./codex-approval-outcome.ts";
import { approvalRun } from "./codex-moved-fixtures.ts";
import { reduceTurnSkeleton } from "./turn-skeleton.ts";

const OPTIONS = [
  { optionId: "a1", effect: "allow", scope: "once", persistentTarget: null },
  { optionId: "d1", effect: "decline", scope: "once", persistentTarget: null },
  { optionId: "a2", effect: "allow", scope: "persistent", persistentTarget: "Bash(x)" },
];
const pending = { requestId: "r1", state: "pendingClientDecision", options: OPTIONS };
const final = (state: string, requestId = "r1") => ({ requestId, state, options: [] });

function readings(state: string, filePresent: boolean): ApprovalReadings {
  return {
    pending: approvalRecordsFrom({ approvals: [pending] })[0]!,
    final: approvalRecordsFrom({ approvals: [final(state)] })[0]!,
    filePresent,
  };
}
const judge = (
  caseName: ApprovalCase,
  cliDecision: string | null,
  state: string,
  filePresent: boolean,
) => evaluateApprovalCase({ case: caseName, cliDecision, readings: readings(state, filePresent) });

test("allow-once is decided with the file present; decline is decided with it absent", () => {
  assert.deepEqual(judge("allow-once", "accept", "decided", true), {
    violations: [],
    agentOutcome: "accept",
  });
  assert.deepEqual(judge("decline", "decline", "decided", false), {
    violations: [],
    agentOutcome: "decline",
  });
});

test("a timeout is timedOut with the file absent and its outcome is timedOut", () => {
  assert.deepEqual(judge("timeout", null, "timedOut", false), {
    violations: [],
    agentOutcome: "timedOut",
  });
});

test("negative: the CLI showed the prompt but the record stays pending", () => {
  const result = judge("allow-once", "accept", "pendingClientDecision", true);
  assert.ok(result.violations.includes("the record stays pending after the answer"));
  assert.match(result.agentOutcome, /^mismatch\(/);
});

test("negative: allow was answered but the file is absent", () => {
  const result = judge("allow-once", "accept", "decided", false);
  assert.deepEqual(result.violations, [
    "the scratch file is absent, but the case requires it present",
  ]);
  assert.match(result.agentOutcome, /^mismatch\(/);
});

test("negative: decline was answered but the file exists", () => {
  assert.deepEqual(judge("decline", "decline", "decided", true).violations, [
    "the scratch file exists, but the case requires it absent",
  ]);
});

test("negative: a timeout reported as decided", () => {
  assert.deepEqual(judge("timeout", null, "decided", false).violations, [
    "record state decided, expected timedOut",
  ]);
});

test("negative: the file exists after a timeout, and a timeout that never ended pending", () => {
  assert.ok(judge("timeout", null, "timedOut", true).violations.length > 0);
  assert.ok(
    judge("timeout", null, "pendingClientDecision", false).violations.includes(
      "the record stays pending after the answer",
    ),
  );
});

test("negative: the prompt record was already decided, or offers no decline-once option", () => {
  const decidedEarly = {
    ...readings("decided", true),
    pending: approvalRecordsFrom({ approvals: [final("decided")] })[0]!,
  };
  const early = evaluateApprovalCase({
    case: "allow-once",
    cliDecision: "accept",
    readings: decidedEarly,
  });
  assert.ok(early.violations.some((violation) => violation.includes("not pendingClientDecision")));
  assert.ok(early.violations.some((violation) => violation.includes("0 allow-once options")));
  const noDecline = {
    ...readings("decided", true),
    pending: approvalRecordsFrom({
      approvals: [{ ...pending, options: OPTIONS.filter((option) => option.effect === "allow") }],
    })[0]!,
  };
  assert.ok(
    evaluateApprovalCase({
      case: "allow-once",
      cliDecision: "accept",
      readings: noDecline,
    }).violations.includes("the record offers 0 decline-once options, not exactly one"),
  );
});

test("negative: the final record is another request", () => {
  const other = {
    ...readings("decided", true),
    final: approvalRecordsFrom({ approvals: [final("decided", "other")] })[0]!,
  };
  assert.deepEqual(
    evaluateApprovalCase({ case: "allow-once", cliDecision: "accept", readings: other }).violations,
    ["the final record is another request than the one the prompt showed"],
  );
});

test("a declared decision that does not fit the case is refused", () => {
  assert.ok(judge("timeout", "accept", "timedOut", false).violations.length > 0);
  assert.ok(judge("allow-once", null, "decided", true).violations.length > 0);
});

test("an answer of another shape is an error, not an empty list", () => {
  assert.throws(() => approvalRecordsFrom({ approvals: [{ state: "decided" }] }));
});

test("outcomes feed k5 in prompt order, except a run that includes a timeout", () => {
  const accepted = approvalOutcomeFile({
    case: "allow-once",
    cliDecision: "accept",
    readings: readings("decided", true),
  });
  const timedOut = approvalOutcomeFile({
    case: "timeout",
    cliDecision: null,
    readings: readings("timedOut", false),
  });
  assert.deepEqual(agentApprovalOutcomesFor([accepted]), ["accept"]);
  assert.equal(agentApprovalOutcomesFor([accepted, timedOut]), null);
});

test("wire: one prompt answered as declared; wrong answer, extra prompt and answered timeout fail", () => {
  const check = (
    frames: ReturnType<typeof approvalRun>,
    name: ApprovalCase,
    decision: string | null,
  ) => checkApprovalAgainstWire(reduceTurnSkeleton(frames), { case: name, cliDecision: decision });
  assert.deepEqual(check(approvalRun({ decision: "accept" }), "allow-once", "accept"), []);
  assert.deepEqual(check(approvalRun(), "timeout", null), []);
  assert.equal(check(approvalRun({ decision: "decline" }), "allow-once", "accept").length, 1);
  assert.equal(
    check(approvalRun({ decision: "accept", prompts: 2 }), "allow-once", "accept").length,
    1,
  );
  assert.equal(check(approvalRun({ prompts: 0 }), "allow-once", "accept").length, 1);
  assert.equal(check(approvalRun({ decision: "accept" }), "timeout", null).length, 1);
});
