// The embedded SQL must be byte-for-byte what its sources hold: main's
// checkout and production's commit through git.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { test } from "node:test";

test("the generated schema sources match main and production", () => {
  // Act and assert: the generator's check mode exits nonzero when stale.
  assert.doesNotThrow(() =>
    execFileSync(
      process.execPath,
      [resolve(import.meta.dirname, "../scripts/generate_schema_sources.ts"), "--check"],
      { stdio: "pipe" },
    ),
  );
});
