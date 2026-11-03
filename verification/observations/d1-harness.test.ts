import assert from "node:assert/strict";
import { test } from "node:test";
import { d1HarnessSchema } from "./d1-harness.ts";

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
