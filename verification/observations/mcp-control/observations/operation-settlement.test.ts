import assert from "node:assert/strict";
import { test } from "node:test";
import { immediateAnswerProblem, settlementState } from "./operation-settlement.ts";

const TARGET = { endpoint: { serviceId: "svc", endpointId: "cursor-local" }, sessionId: "s-1" };
const OTHER = { ...TARGET, endpoint: { ...TARGET.endpoint, endpointId: "claude-local" } };
const snapshot = (fields: Readonly<Record<string, unknown>>): unknown => ({
  operationId: "op-1",
  operation: "conversationClose",
  target: TARGET,
  stage: "terminal",
  effect: "applied",
  reconciliation: "confirmed",
  ...fields,
});

test("only a terminal, applied, confirmed record on the exact target is settled", () => {
  assert.deepEqual(settlementState(snapshot({}), "op-1", TARGET), { kind: "settled" });
  assert.deepEqual(settlementState(snapshot({ stage: "dispatched" }), "op-1", TARGET), {
    kind: "open",
    stage: "dispatched",
  });
  assert.deepEqual(settlementState(snapshot({ effect: "notApplied" }), "op-1", TARGET), {
    kind: "refused",
    reason: "the operation ended notApplied/confirmed",
  });
  assert.equal(settlementState(snapshot({ target: OTHER }), "op-1", TARGET).kind, "refused");
  assert.equal(settlementState(snapshot({ operationId: "op-2" }), "op-1", TARGET).kind, "refused");
});

test("an admission for another operation or Session is refused before any follow-up", () => {
  const submission = (operation: unknown): unknown => ({ admission: "admitted", operation });
  assert.equal(
    immediateAnswerProblem(submission(snapshot({ stage: "admitted" })), "op-1", TARGET),
    null,
  );
  assert.equal(
    immediateAnswerProblem(submission(snapshot({ target: OTHER })), "op-1", TARGET),
    "the answer names another Session",
  );
  assert.equal(
    immediateAnswerProblem(submission(snapshot({ operationId: "op-9" })), "op-1", TARGET),
    "the answer names another operation",
  );
  assert.equal(
    immediateAnswerProblem(
      { kind: "pending", operationId: "op-1", target: TARGET },
      "op-1",
      TARGET,
    ),
    null,
  );
  assert.equal(
    immediateAnswerProblem({ unexpected: true }, "op-1", TARGET),
    "the answer is neither a submission nor an operation result",
  );
});
