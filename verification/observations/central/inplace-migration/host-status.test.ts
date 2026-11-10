import { test } from "node:test";
import assert from "node:assert/strict";
import { qualifyConnectedHost } from "./host-status.ts";
const peer = "01900000-0000-7000-8000-000000000001";
const expected = {
  peer,
  origin: "https://staging.example.test",
  endpoint: "https://staging.example.test/v1/session",
};
const status = `result: ready\ncentral: connected since 2026-11-09 00:00:00 UTC; last message from central 2026-11-09 00:01:00 UTC; instance 9007199254740993; connection 4; revision 5\ncentral peer: ${peer}; endpoint: https://staging.example.test/v1/session\n`;
test("actual Host connected-line shape preserves exact counters and checks peer/origin", (): void => {
  assert.equal(qualifyConnectedHost(status, expected).instance, 9007199254740993n);
  assert.equal(qualifyConnectedHost(status, expected).endpoint, expected.endpoint);
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

test("connected Host must preserve the exact enrolled path and refuse query, fragment or credentials", (): void => {
  for (const endpoint of [
    "https://staging.example.test/",
    "https://staging.example.test/v1/other",
    "https://staging.example.test/v1/session?token=unexpected",
    "https://staging.example.test/v1/session#unexpected",
    "https://user:password@staging.example.test/v1/session",
    "https://production.example.test/v1/session",
  ])
    assert.throws(
      () => qualifyConnectedHost(status.replace(expected.endpoint, endpoint), expected),
      /mismatch/,
    );
  assert.throws(
    () =>
      qualifyConnectedHost(status, { ...expected, endpoint: `${expected.endpoint}?unexpected` }),
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
