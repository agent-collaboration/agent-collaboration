import assert from "node:assert/strict";
import { test } from "node:test";
import { assertHeartbeatDidNotWake, assertRestoredFrame } from "./durable-object-hibernation.ts";

const first = {
  kind: "observed",
  instance: "01890f3e-7b8c-7cc0-8000-000000000001",
  attachment: { witness: "01890f3e-7b8c-7cc0-8000-000000000002" },
  count: 1,
};
const second = {
  ...first,
  instance: "01890f3e-7b8c-7cc0-8000-000000000003",
  count: 2,
};

test("heartbeat oracle rejects either constructor or handler execution", (): void => {
  const before = { constructors: 1, handlers: 1 };
  assertHeartbeatDidNotWake(before, before);
  assert.throws((): void => assertHeartbeatDidNotWake(before, { constructors: 2, handlers: 1 }));
  assert.throws((): void => assertHeartbeatDidNotWake(before, { constructors: 1, handlers: 2 }));
});

test("restoration requires new instance, exact attachment and surviving store", (): void => {
  const verify = (after: unknown): void =>
    assertRestoredFrame({ before: first, after, witness: first.attachment.witness });
  verify(second);
  assert.throws((): void => verify({ ...second, instance: first.instance }));
  assert.throws((): void => verify({ ...second, attachment: { witness: second.instance } }));
  assert.throws((): void => verify({ ...second, count: 1 }));
  assert.throws((): void => verify({ ...second, attachment: null }));
});
