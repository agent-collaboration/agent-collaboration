import assert from "node:assert/strict";
import { test } from "node:test";
import { requirePreservedRootFiles } from "./installation-root-update.ts";

test("root update cannot lose or replace enrollment, identity or project bytes", (): void => {
  const before = { identity: "a".repeat(64), enrollment: "b".repeat(64), project: "c".repeat(64) };
  assert.doesNotThrow(() => requirePreservedRootFiles(before, { ...before }));
  assert.throws(
    () => requirePreservedRootFiles(before, { ...before, enrollment: "d".repeat(64) }),
    /enrollment or project files/,
  );
  assert.throws(
    () =>
      requirePreservedRootFiles(before, {
        identity: before.identity,
        enrollment: before.enrollment,
      }),
    /enrollment or project files/,
  );
});
