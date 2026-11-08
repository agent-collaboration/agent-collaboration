import assert from "node:assert/strict";
import { test } from "node:test";
import { d1HarnessSchema } from "./d1-harness-plan.ts";
import { runD1Harness } from "./d1-harness.ts";
import type { D1HarnessJourney } from "./d1-harness-plan.ts";

const artifact = {
  path: "/declared/executable",
  sha256: "a".repeat(64),
  identifier: "declared.debug",
  team: "declared",
};
const declaration = {
  suite: "d1-harness",
  repository: "/declared/worktree",
  driver: artifact,
  cli: artifact,
  native: { path: "/declared/native", sha256: "b".repeat(64), version: "codex-cli declared" },
  evidenceDirectory: "/declared/worktree/tmp/d1-verify/evidence",
  debugProxyOrigin: "http://127.0.0.1:18787",
  deadline: "2033-05-18T03:33:20Z",
};

test("D1 harness declaration keeps debug routing distinct from production and credential carriers", (): void => {
  d1HarnessSchema.parse(declaration);
  for (const debugProxyOrigin of [
    "http://127.0.0.1:19741",
    "http://127.0.0.1:19742",
    "http://example.com:18787",
    "http://user:secret@127.0.0.1:18787",
    "http://127.0.0.1:18787?token=forbidden",
  ])
    assert.equal(d1HarnessSchema.safeParse({ ...declaration, debugProxyOrigin }).success, false);
});

test("D1 startup harness cannot enable a model turn or take an unqualified executable", (): void => {
  assert.equal(d1HarnessSchema.safeParse({ ...declaration, allowModelInput: true }).success, false);
  assert.equal(
    d1HarnessSchema.safeParse({ ...declaration, driver: { ...artifact, sha256: "unknown" } })
      .success,
    false,
  );
});

test("D1 harness accepts an explicit short runtime allocation without widening other roots", (): void => {
  assert.equal(
    d1HarnessSchema.parse({ ...declaration, runtimeDirectory: "/private/tmp/d1v-abc123" })
      .runtimeDirectory,
    "/private/tmp/d1v-abc123",
  );
  for (const runtimeDirectory of [
    "/private/tmp/other",
    "/tmp/d1v-abc123",
    "/private/tmp/d1v-abc123/../escape",
  ])
    assert.equal(d1HarnessSchema.safeParse({ ...declaration, runtimeDirectory }).success, false);
});

test("materialization cannot release twenty queued messages or select a different proxy or journey", (): void => {
  const materialization = {
    kind: "materialized-luna-recipient",
    budgetFile: "/declared/worktree/tmp/d1-verify/budget.json",
    reloadHeld: false,
  };
  const valid = {
    ...declaration,
    queueScenario: "online-timing",
    debugProxyOrigin: "http://127.0.0.1:43127",
    materialization,
  };
  assert.equal(d1HarnessSchema.safeParse(valid).success, true);
  for (const invalid of [
    { ...valid, materialization: { ...materialization, reloadHeld: true } },
    { ...valid, debugProxyOrigin: "http://127.0.0.1:28787" },
    { ...valid, queueScenario: "offline-reopen" },
    { ...valid, twoProjects: true },
  ])
    assert.equal(d1HarnessSchema.safeParse(invalid).success, false);
});

test("a custom no-model journey refuses legacy selectors before evidence IO or launch", async (): Promise<void> => {
  // These paths do not exist: a late selector check would attempt realpath first.
  const uncalled = (): never => {
    throw new Error("journey callback must not run");
  };
  const journey: D1HarnessJourney = {
    declaration: {},
    centralStarted: uncalled,
    sessionObservation: uncalled,
    connectionReady: uncalled,
    recipientStarted: uncalled,
    drive: uncalled,
    ownersStopped: uncalled,
  };
  const cases = [
    { ...declaration, queueScenario: "online-held" },
    { ...declaration, twoProjects: true },
    {
      ...declaration,
      queueScenario: "online-timing",
      debugProxyOrigin: "http://127.0.0.1:43127",
      materialization: {
        kind: "materialized-luna-recipient",
        budgetFile: "/absent/budget.json",
        reloadHeld: false,
      },
    },
  ];
  for (const candidate of cases)
    await assert.rejects(
      runD1Harness(d1HarnessSchema.parse(candidate), journey),
      /cannot combine legacy/,
    );
});
