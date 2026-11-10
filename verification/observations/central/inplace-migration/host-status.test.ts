import { test } from "node:test";
import assert from "node:assert/strict";
import { qualifyConnectedHost } from "./host-status.ts";
const peer = "01900000-0000-7000-8000-000000000001";
const expected = { peer, origin: "https://staging.example.test" };
const status = `result: ready\ncentral: connected since 2026-11-09 00:00:00 UTC; last message from central 2026-11-09 00:01:00 UTC; instance 9007199254740993; connection 4; revision 5\ncentral peer: ${peer}; endpoint: https://staging.example.test\n`;
test("actual Host connected-line shape preserves exact counters and checks peer/origin", (): void => {
  assert.equal(qualifyConnectedHost(status, expected).instance, 9007199254740993n);
  assert.throws(
    () =>
      qualifyConnectedHost(status, { ...expected, peer: "01900000-0000-7000-8000-000000000002" }),
    /mismatch/,
  );
  assert.throws(
    () => qualifyConnectedHost(status, { ...expected, origin: "https://production.example.test" }),
    /mismatch/,
  );
});
test("disconnected, missing, duplicate and historical connected text do not qualify", (): void => {
  for (const text of [
    status.replace("central: connected", "central: disconnected"),
    "result: ready\n",
    `${status}${status}`,
    status.replace("central: connected", "previous central: connected"),
  ])
    assert.throws(() => qualifyConnectedHost(text, expected));
});
