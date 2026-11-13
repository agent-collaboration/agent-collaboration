import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";

const entry = new URL("./run.ts", import.meta.url).pathname;
const unavailable = /Two-Mac verification harness unavailable until deliverable C/;

test("two-Mac entry refuses before reading a declaration or launching setup", (): void => {
  const result = spawnSync(process.execPath, [entry, "/absent-deliverable-c/declaration.json"], {
    encoding: "utf8",
  });
  assert.equal(result.status, 2, result.stderr);
  assert.match(result.stderr, unavailable);
  assert.equal(result.stdout, "");
});

test("help names the unavailable two-Mac route and retains D1 entry points", (): void => {
  const result = spawnSync(process.execPath, [entry, "--help"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, unavailable);
  assert.match(result.stdout, /--d1-harness/);
  assert.match(result.stdout, /--d1-readback/);
});

test("D1 plan remains available without starting a runtime", (): void => {
  const result = spawnSync(process.execPath, [entry, "--d1-plan"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const plan: unknown = JSON.parse(result.stdout);
  assert.equal(typeof plan, "object");
  assert.match(result.stdout, /"executed":false/);
  assert.match(result.stdout, /"suite":"d1-central-layout-and-mail"/);
});
