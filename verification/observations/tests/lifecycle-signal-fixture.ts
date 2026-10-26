import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { LifecycleFixture } from "./lifecycle-fixture.ts";

const root = process.argv[2];
assert.ok(root);
const fixture = new LifecycleFixture(root, "interrupt");
const error = await fixture.drive();
writeFileSync(
  join(root, "driver-finished.json"),
  JSON.stringify({ failure: error?.constructor.name ?? null }),
);
process.exitCode = error?.constructor.name === "KeyboardInterrupt" ? 130 : 1;
