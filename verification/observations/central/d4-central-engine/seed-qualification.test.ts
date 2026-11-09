import { test } from "node:test";
import assert from "node:assert/strict";
import { assessSeedFailure } from "./seed-qualification.ts";

function positive(): unknown {
  return {
    journey: "J17",
    scenario: "full-window-silent-peer",
    shell: "tokio",
    sourceRoot: "/private/tmp/d4-positive",
    baseProductHead: "a".repeat(40),
    artifactSHA256: "b".repeat(64),
    oracleSHA256: "c".repeat(64),
    configurationSHA256: "d".repeat(64),
    reached: ["recipient-welcomed", "200-items-accepted", "answers-held"],
    checked: ["window-at-most-32"],
    outcome: { kind: "passed" },
    cleanup: "settled",
  };
}
function seeded(): Record<string, unknown> {
  const base = positive();
  if (typeof base !== "object" || base === null) throw new Error("fixture");
  return {
    ...base,
    sourceRoot: "/private/tmp/d4-seed",
    artifactSHA256: "e".repeat(64),
    outcome: { kind: "assertionFailed", assertion: "window-at-most-32", actual: 33, expected: 32 },
  };
}
const expected = {
  journey: "J17",
  scenario: "full-window-silent-peer",
  shell: "tokio",
  assertion: "window-at-most-32",
  readiness: ["recipient-welcomed", "200-items-accepted", "answers-held"],
};

test("a reached semantic failure matches its exact check while retaining the limits of receipt-only evidence", (): void => {
  assert.deepEqual(
    assessSeedFailure({ restored: positive(), expected, positive: positive(), seeded: seeded() }),
    {
      kind: "semanticFailureMatched",
      assertion: "window-at-most-32",
      runtimeQualified: false,
      remaining: [
        "source-copy and patch custody",
        "signed artifact provenance",
        "original live observation evidence",
      ],
    },
  );
});

test("an import, build, timeout, missing test, or early transport error never counts as a caught wrong build", (): void => {
  for (const outcome of [
    { kind: "harnessFailed", error: "ERR_MODULE_NOT_FOUND" },
    { kind: "harnessFailed", error: "cargo build exit101" },
    { kind: "harnessFailed", error: "timeout before readiness" },
    { kind: "unavailable", reason: "zero matching tests" },
    { kind: "passed" },
  ]) {
    const result = assessSeedFailure({
      restored: positive(),
      expected,
      positive: positive(),
      seeded: { ...seeded(), outcome },
    });
    assert.equal(result.kind, "unknown");
  }
});

test("a different assertion or an unreached fault does not establish sensitivity", (): void => {
  for (const change of [
    {
      outcome: {
        kind: "assertionFailed",
        assertion: "host-died-before-ready",
        actual: false,
        expected: true,
      },
    },
    { reached: ["recipient-welcomed", "200-items-accepted"] },
    { checked: [] },
    { cleanup: "unverified" },
  ]) {
    const result = assessSeedFailure({
      restored: positive(),
      expected,
      positive: positive(),
      seeded: { ...seeded(), ...change },
    });
    assert.equal(result.kind, "unknown");
  }
});

test("unrelated cases, oracles, configurations, builds, or roots cannot be compared as a positive and its seed", (): void => {
  for (const change of [
    { journey: "J6b" },
    { scenario: "other-case" },
    { shell: "workerd" },
    { baseProductHead: "f".repeat(40) },
    { oracleSHA256: "f".repeat(64) },
    { configurationSHA256: "f".repeat(64) },
    { artifactSHA256: "b".repeat(64) },
    { sourceRoot: "/private/tmp/d4-positive" },
    { sourceRoot: "/private/tmp/d4-positive/nested-seed" },
    { sourceRoot: "/private/tmp/d4-positive/../d4-seed" },
  ]) {
    const result = assessSeedFailure({
      restored: positive(),
      expected,
      positive: positive(),
      seeded: { ...seeded(), ...change },
    });
    assert.equal(result.kind, "unknown");
  }
});

test("a positive missing the same check, readiness or settlement cannot qualify a later seed failure", (): void => {
  for (const change of [
    { checked: [] },
    { reached: [] },
    { cleanup: "unverified" },
    {
      outcome: {
        kind: "assertionFailed",
        assertion: "window-at-most-32",
        actual: 33,
        expected: 32,
      },
    },
  ]) {
    const base = positive();
    if (typeof base !== "object" || base === null) throw new Error("fixture");
    const result = assessSeedFailure({
      restored: positive(),
      expected,
      positive: { ...base, ...change },
      seeded: seeded(),
    });
    assert.equal(result.kind, "unknown");
  }
});

test("a missing or changed restored positive, or equal actual/expected failure, stays unqualified", (): void => {
  const original = positive();
  if (typeof original !== "object" || original === null) throw new Error("fixture");
  for (const restored of [
    undefined,
    { ...original, artifactSHA256: "f".repeat(64) },
    { ...original, reached: [] },
  ]) {
    assert.equal(
      assessSeedFailure({ expected, positive: original, seeded: seeded(), restored }).kind,
      "unknown",
    );
  }
  assert.equal(
    assessSeedFailure({
      expected,
      positive: original,
      restored: original,
      seeded: {
        ...seeded(),
        outcome: {
          kind: "assertionFailed",
          assertion: "window-at-most-32",
          actual: 32,
          expected: 32,
        },
      },
    }).kind,
    "unknown",
  );
});
