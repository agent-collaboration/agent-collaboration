import assert from "node:assert/strict";
import { test } from "node:test";
import { inspectReapplyReadback } from "./restore-mode-reapply.ts";

test("the unconfirmed restore reports the fallback in options without confirming the setter", () => {
  const inspected = {
    currentMode: null,
    configOptions: [
      { id: "mode", category: "mode", currentValue: { kind: "select", value: "acceptEdits" } },
    ],
  };
  assert.deepEqual(inspectReapplyReadback(inspected, undefined, "acceptEdits", true), {
    reported: "acceptEdits",
    agrees: true,
  });
  assert.equal(inspectReapplyReadback(inspected, "default", "acceptEdits", true).agrees, false);
  assert.equal(inspectReapplyReadback(inspected, undefined, "default", true).agrees, false);
  assert.equal(inspectReapplyReadback(inspected, undefined, "acceptEdits", false).agrees, false);
});
