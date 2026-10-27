import assert from "node:assert/strict";
import { closeSync, openSync, writeFileSync } from "node:fs";

const [, , observationPath, executable, ...arguments_] = process.argv;
assert.ok(observationPath !== undefined && executable !== undefined);
const descriptor = openSync(observationPath, "w");
try {
  const token = process.env.ANTHROPIC_AUTH_TOKEN;
  assert.ok(token !== undefined);
  writeFileSync(descriptor, token);
} finally {
  closeSync(descriptor);
}
assert.ok(process.execve !== undefined, "native exec is required by this Unix fixture");
process.execve(executable, [executable, ...arguments_], process.env);
