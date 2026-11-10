import { test } from "node:test";
import assert from "node:assert/strict";
import { validateRetainedWriter } from "./retained-writer.ts";
const project = "01900000-0000-7000-8000-000000000001",
  machine = "01900000-0000-7000-8000-000000000002";
function original(): unknown {
  return {
    kind: "retainedWriterObserved",
    database: project,
    holder: machine,
    lease: 3n,
    originEpoch: 3n,
    originStart: { epoch: 3n, sequence: 0n },
    originalRegistration: { database: project, holder: machine, epoch: 3n, state: "held" },
    retainedAuthority: "noObservedLoss",
    sourceSha256: "a".repeat(64),
    currentRenewalObserved: false,
  };
}
test("retained reader tuple is bound to source project and machine; never current renewal credit", (): void => {
  assert.deepEqual(
    validateRetainedWriter({
      raw: original(),
      expectedProject: project,
      expectedMachine: machine,
      sourceSha256: "a".repeat(64),
    }),
    { database: project, holder: machine, lease: 3n },
  );
});
test("seeded wrong lease and forged hash or current-renewal claim fail", (): void => {
  const raw = original();
  assert.ok(typeof raw === "object" && raw !== null);
  for (const changed of [
    { ...raw, lease: 4n },
    { ...raw, originEpoch: 4n },
    { ...raw, sourceSha256: "b".repeat(64) },
    { ...raw, currentRenewalObserved: true },
  ])
    assert.throws((): void => {
      validateRetainedWriter({
        raw: changed,
        expectedProject: project,
        expectedMachine: machine,
        sourceSha256: "a".repeat(64),
      });
    });
});
