import test from "node:test";
import assert from "node:assert/strict";
import template from "../cloudflare.config.ts";

test("the public deployment template binds each workspace store to its SQLite object class", () => {
  assert.deepEqual(template.worker.env.WORKSPACE_STORE, {
    type: "durable-object",
    worker: "replace-worker-name",
    exportName: "WorkspaceStoreObject",
  });
  assert.deepEqual(template.worker.exports.WorkspaceStoreObject, {
    type: "durable-object",
    storage: "sqlite",
  });
});
