// Sequencing sensitivity for callerLoss followed by explicitCancel on the same controlled peer:
// a retired marker lets the next hold start; a marker left behind stops it before the hold.
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import {
  releaseHeldWork,
  requireNoReleaseMarker,
  retireReleaseMarker,
} from "./release-sentinel.ts";

function withSentinel(body: (sentinel: string) => void): void {
  const directory = mkdtempSync(join(tmpdir(), "release-sentinel-"));
  try {
    body(join(directory, "release"));
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

test("a released and retired marker lets the next controlled hold start", () => {
  withSentinel((sentinel) => {
    requireNoReleaseMarker(sentinel);
    releaseHeldWork(sentinel, "op-caller-loss");
    retireReleaseMarker(sentinel, "op-caller-loss");
    assert.equal(existsSync(sentinel), false);
    assert.doesNotThrow(() => requireNoReleaseMarker(sentinel));
  });
});

test("a marker left behind stops the next hold before it starts", () => {
  withSentinel((sentinel) => {
    releaseHeldWork(sentinel, "op-caller-loss");
    assert.throws(() => requireNoReleaseMarker(sentinel), /would be released at once/);
    assert.throws(() => releaseHeldWork(sentinel, "op-explicit-cancel"), /EEXIST/);
  });
});

test("a marker this run did not write is never removed", () => {
  withSentinel((sentinel) => {
    writeFileSync(sentinel, "someone else\n");
    assert.throws(() => retireReleaseMarker(sentinel, "op-caller-loss"), /did not write/);
    assert.equal(existsSync(sentinel), true);
    assert.throws(
      () => retireReleaseMarker(join(sentinel, "..", "absent"), "op-caller-loss"),
      /already gone/,
    );
  });
});
