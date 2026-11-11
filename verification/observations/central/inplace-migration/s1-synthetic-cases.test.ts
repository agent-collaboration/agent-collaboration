import assert from "node:assert/strict";
import { test } from "node:test";
import { classifySyntheticWrite, requireSyntheticScope } from "./s1-synthetic-cli.ts";

test("old125 normalized write receipts distinguish acceptance from pending and refusal", (): void => {
  assert.equal(
    classifySyntheticWrite({
      record: { result: { kind: "decided", decision: { kind: "accepted" } } },
    }),
    "accepted",
  );
  assert.equal(
    classifySyntheticWrite({ record: { result: { kind: "projectCreationPending" } } }),
    "pending",
  );
  assert.equal(
    classifySyntheticWrite({
      record: { result: { kind: "decided", decision: { kind: "rejected" } } },
    }),
    "stop",
  );
  assert.equal(
    classifySyntheticWrite({ result: { kind: "decided", decision: { kind: "accepted" } } }),
    "stop",
  );
});

test("synthetic mutation scope rejects imported projects even when accidentally allowlisted", (): void => {
  const imported = "01a14e9e-0000-7000-8000-000000000000";
  assert.throws(
    (): void =>
      requireSyntheticScope(["task", "create", "--project-id", imported], new Set([imported])),
    /synthetic scope/,
  );
});
test("synthetic scope requires every workspace, board and project to be allocated by this run", (): void => {
  const workspace = "01a1c300-0000-7000-8000-000000000000";
  const board = "01a1c300-0000-7000-8000-000000000001";
  assert.doesNotThrow((): void =>
    requireSyntheticScope(
      ["board", "create", "--workspace-id", workspace, "--board-id", board],
      new Set([workspace, board]),
    ),
  );
  assert.throws(
    (): void =>
      requireSyntheticScope(
        ["board", "create", "--workspace-id", workspace, "--board-id", board],
        new Set([workspace]),
      ),
    /synthetic scope/,
  );
});
test("synthetic scope forbids enrollment, claim, repair and transfer commands", (): void => {
  for (const verb of ["enroll", "claim", "move", "takeover", "repair", "migrate"])
    assert.throws((): void => requireSyntheticScope(["project", verb], new Set()), /commission/);
});
