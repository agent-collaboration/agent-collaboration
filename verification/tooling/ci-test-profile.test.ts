import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { inventoryCiTests, profileCiTests } from "./ci-test-profile.ts";

const mixedLog = readFileSync(new URL("./fixtures/ci-test-profile.txt", import.meta.url), "utf8");

test("concurrent invocations keep FAIL results and use their own Summary, ignoring final reprints", (): void => {
  const result = inventoryCiTests(mixedLog);
  assert.equal(result.kind, "inventoried");
  if (result.kind !== "inventoried") return;
  assert.deepEqual(
    result.invocations.map(({ inventory }) => [
      inventory.runTotal,
      inventory.outcomes.length,
      inventory.summaryPassed,
      inventory.summaryFailed,
      inventory.summarySkipped,
    ]),
    [
      [4, 4, 3, 1, 2],
      [2, 2, 2, 0, 0],
    ],
  );
  assert.equal(result.invocations[0]?.inventory.outcomes[2]?.result, "fail");
});

const failures = [
  {
    name: "dropping a FAIL line is an inventory mismatch",
    log: mixedLog.replace(/^.*FAIL.*\n/m, ""),
    expected: { kind: "inventoryMismatch", runTotal: 4, counted: 3, summaryRun: 4 },
  },
  {
    name: "missing result differs from its own Summary",
    log: mixedLog.replace(/^.*PASS.*workspace::lib fourth.*\n/m, ""),
    expected: { kind: "inventoryMismatch", runTotal: 4, counted: 3, summaryRun: 4 },
  },
  {
    name: "missing Summary cannot produce a profile",
    log: "2026-01-01T00:00:01Z PASS [ 1.000s] (1/1) workspace::lib first\n",
    expected: { kind: "noTimings", reason: "summaryNotFound" },
  },
  {
    name: "no result lines cannot produce a profile",
    log: "unrelated build output\n",
    expected: { kind: "noTimings", reason: "logMissingResultLines" },
  },
  {
    name: "two Summaries with equal totals are ambiguous",
    log: `${mixedLog}2026-01-01T00:00:10Z Summary [ 2.000s] 2 tests run: 2 passed, 0 skipped\n`,
    expected: { kind: "ambiguousInvocations", runTotal: 2 },
  },
  {
    name: "concurrent invocations with equal totals are ambiguous before their Summaries",
    log: mixedLog.replace("Starting 2 tests", "Starting 4 tests"),
    expected: { kind: "ambiguousInvocations", runTotal: 4 },
  },
  {
    name: "incorrect PASS and FAIL counts cannot hide behind a matching run count",
    log: mixedLog.replace("3 passed, 1 failed", "4 passed, 0 failed"),
    expected: { kind: "inventoryMismatch", runTotal: 4, counted: 4, summaryRun: 4 },
  },
];
for (const scenario of failures) {
  test(scenario.name, (): void => {
    assert.deepEqual(profileCiTests(scenario.log), scenario.expected);
  });
}

test("overlapping intervals yield literal concurrency minutes and solo owners", (): void => {
  const result = profileCiTests(mixedLog);
  assert.equal(result.kind, "profiled");
  if (result.kind !== "profiled") return;
  const workspace = result.invocations.find((invocation) => invocation.runTotal === 4);
  assert.ok(workspace);
  assert.equal(workspace.wallMinutes, 8 / 60);
  assert.equal(workspace.elapsedTestMinutes, 11 / 60);
  assert.deepEqual(
    workspace.minutesAtConcurrency,
    new Map([
      [1, 5 / 60],
      [2, 3 / 60],
    ]),
  );
  assert.deepEqual(workspace.soloOwners, [
    { binaryId: "workspace::lib", testName: "third", soloSeconds: 3 },
    { binaryId: "workspace::lib", testName: "first", soloSeconds: 1 },
    { binaryId: "workspace::lib", testName: "second", soloSeconds: 1 },
  ]);
});

test("a result without a run counter raises the plan stop condition", (): void => {
  assert.throws(
    () => profileCiTests(mixedLog.replace("(1/4)", "")),
    /result line has no run counter/,
  );
});
