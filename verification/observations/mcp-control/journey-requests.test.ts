// Producer/consumer check for operation IDs: the consumer's own rule is read from its Rust source
// (agent-automation automation_identity.rs), so a change on either side fails here rather than as
// a refused live call.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { newOperationId } from "./journey-requests.ts";

const repository = join(dirname(fileURLToPath(import.meta.url)), "../../..");

function consumerPattern(): RegExp {
  const source = readFileSync(
    join(repository, "crates/agent-automation/src/automation_identity.rs"),
    "utf8",
  );
  const pattern = source.match(/"pattern":"([^"]+)"/)?.[1];
  if (pattern === undefined) throw new Error("the consumer's UUIDv7 pattern is not in its source");
  assert.ok(source.includes("get_version_num() != 7"), "the consumer no longer requires version 7");
  return new RegExp(pattern);
}

test("every produced operation ID is the canonical UUIDv7 the carrier tools accept", () => {
  const pattern = consumerPattern();
  const produced = Array.from({ length: 200 }, () => newOperationId());
  for (const id of produced) {
    assert.match(id, pattern);
    assert.equal(id, id.toLowerCase());
  }
  assert.equal(new Set(produced).size, produced.length);
});

test("a version-4 UUID, as crypto.randomUUID makes, is refused by the consumer's rule", () => {
  assert.doesNotMatch("0f1e2d3c-4b5a-4987-8a76-5f4e3d2c1b0a", consumerPattern());
});
