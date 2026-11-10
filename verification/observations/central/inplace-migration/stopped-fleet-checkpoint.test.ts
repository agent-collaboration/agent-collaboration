import assert from "node:assert/strict";
import { test } from "node:test";
import { decodeAuthoredJson } from "../../../exact-json.ts";
import {
  stoppedFleetCheckpointPlan,
  validateStoppedCheckpoint,
} from "./stopped-fleet-checkpoint.ts";

test("the controller's authored JSON checkpoint plan keeps its exact safe head", (): void => {
  // Arrange: the same JSON number the controller reads from an operator plan.
  const plan = {
    root: "/private/tmp/2026-11-s1-checkpoint-test/router",
    oldHostVersion: "0.1.125",
    reader: {
      path: "/private/tmp/2026-11-s1-checkpoint-test/reader",
      sha256: "a".repeat(64),
      identifier: "dev.shravansunder.test.reader",
      team: "974QD84WVC",
    },
    evidenceDirectory: "/private/tmp/2026-11-s1-checkpoint-test/evidence",
    centralAtCut: {
      origin: "https://s1-migration.agent-collaboration.com",
      sourceHead: "a".repeat(40),
    },
    migrationInventory: {
      artifact: "/private/tmp/2026-11-s1-checkpoint-test/inventory.json",
      sha256: "b".repeat(64),
      oldFleetHead: 136,
      oldCentralSourceHead: "c".repeat(40),
    },
  };

  // Act: preserve the real authored-JSON decoder and the consumer's schema.
  const observed = stoppedFleetCheckpointPlan.parse(decodeAuthoredJson(JSON.stringify(plan)));

  // Assert: revalidation also accepts the exact domain value; unsafe inputs fail.
  assert.equal(observed.migrationInventory.oldFleetHead, 136n);
  assert.equal(stoppedFleetCheckpointPlan.parse(observed).migrationInventory.oldFleetHead, 136n);
  for (const oldFleetHead of [Number.MAX_SAFE_INTEGER + 1, -1, 1.5, "136"]) {
    assert.throws(() =>
      stoppedFleetCheckpointPlan.parse(
        decodeAuthoredJson(
          JSON.stringify({
            ...plan,
            migrationInventory: { ...plan.migrationInventory, oldFleetHead },
          }),
        ),
      ),
    );
  }
});

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
