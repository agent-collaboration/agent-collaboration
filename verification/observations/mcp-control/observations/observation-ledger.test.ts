// A baseline that spans several result-limited reads: its events are the baseline, not
// post-baseline history, and the cursor chain across all reads is still checked.
import assert from "node:assert/strict";
import { test } from "node:test";
import { boundedObservationResultSchema, type SessionRef } from "./observation-models.ts";
import { foldObservationReads, type ObservationRead } from "./observation-ledger.ts";

const TARGET: SessionRef = {
  endpoint: { serviceId: "svc-1", endpointId: "cursor-local" },
  sessionId: "session-1",
};
const event = (sequence: number): unknown => ({
  sequence: BigInt(sequence),
  event: { kind: "stateChanged", state: { kind: "idle" } },
});

function read(
  after: number | null,
  sequences: readonly number[],
  limited: boolean,
): ObservationRead {
  return {
    request: {
      epoch: after === null ? null : 4n,
      afterSequence: after === null ? null : BigInt(after),
    },
    result: boundedObservationResultSchema.parse({
      target: TARGET,
      generation: {},
      attached: true,
      events: sequences.map(event),
      endReason: limited ? "resultLimitReached" : "deadlineReached",
      continuationGap: true,
      epoch: 4n,
    }),
    notifications: [],
  };
}

test("a baseline of several reads is excluded from post-baseline history and kept as baseline events", () => {
  const reads = [read(null, [1, 2], true), read(2, [3, 4], false), read(4, [5], false)];
  const ledger = foldObservationReads(TARGET, reads, 2);
  assert.equal(ledger.kind, "continuous");
  if (ledger.kind !== "continuous") return;
  assert.deepEqual(
    ledger.baselineEvents.map((numbered) => numbered.sequence),
    [1n, 2n, 3n, 4n],
  );
  assert.deepEqual(
    ledger.events.map((numbered) => numbered.sequence),
    [5n],
  );
  assert.equal(ledger.baselineSequence, 4n);
});

test("the default single-read baseline is unchanged, and a broken chain inside the baseline is refused", () => {
  const ledger = foldObservationReads(TARGET, [read(null, [1, 2], false), read(2, [3], false)]);
  assert.equal(ledger.kind, "continuous");
  if (ledger.kind === "continuous") {
    assert.deepEqual(
      ledger.events.map((numbered) => numbered.sequence),
      [3n],
    );
    assert.deepEqual(
      ledger.baselineEvents.map((numbered) => numbered.sequence),
      [1n, 2n],
    );
  }
  const skipped = foldObservationReads(TARGET, [read(null, [1, 2], true), read(3, [4], false)], 2);
  assert.equal(skipped.kind, "rejected");
});
