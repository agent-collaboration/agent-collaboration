import assert from "node:assert/strict";
import { test } from "node:test";
import { validateStoppedCheckpoint } from "./stopped-fleet-checkpoint.ts";

test("stopped Fleet evidence requires a nonzero public checkpoint and identical sealed-B hashes", (): void => {
  const fact = {
    kind: "fleetCheckpointObserved",
    checkpoint: 17n,
    nonzero: true,
    untouchedBeforeSha256: "sealed",
    untouchedAfterSha256: "sealed",
  };
  assert.equal(validateStoppedCheckpoint(fact, "sealed").checkpoint, 17n);
  assert.throws(() =>
    validateStoppedCheckpoint({ ...fact, checkpoint: 0n, nonzero: false }, "sealed"),
  );
  assert.throws(
    () => validateStoppedCheckpoint({ ...fact, untouchedAfterSha256: "changed" }, "sealed"),
    /sealed Fleet B/,
  );
});
