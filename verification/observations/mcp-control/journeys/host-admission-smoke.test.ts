import assert from "node:assert/strict";
import { test } from "node:test";
import { sameExternalBinding } from "./host-admission-smoke.ts";

test("binding identity compares the complete exact-integer generation", () => {
  const admitted = {
    bindingId: "shared-provider-binding",
    generation: { serviceEpoch: "host-a", generation: 9007199254740992n },
  };
  const same = {
    bindingId: "shared-provider-binding",
    generation: { generation: 9007199254740992n, serviceEpoch: "host-a" },
  };
  const nextGeneration = {
    ...admitted,
    generation: { ...admitted.generation, generation: 9007199254740993n },
  };
  const nextEpoch = {
    ...admitted,
    generation: { ...admitted.generation, serviceEpoch: "host-b" },
  };

  assert.equal(sameExternalBinding(admitted, same), true);
  assert.equal(sameExternalBinding(admitted, nextGeneration), false);
  assert.equal(sameExternalBinding(admitted, nextEpoch), false);
  assert.equal(sameExternalBinding(admitted, { ...admitted, bindingId: "other-binding" }), false);
});
