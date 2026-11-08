import { test } from "node:test";
import assert from "node:assert/strict";
import { encode } from "../../../exact-json.ts";
import { D4WorkerdConsole } from "./console-observation.ts";

const object = { kind: "fleet" };
const construction = "01900000-0000-7000-8000-000000000001";
const isolate = "01900000-0000-7000-8000-000000000002";
const created = { centralConstruction: { object, construction, cause: "isolate", isolate } };
const event = {
  centralEvent: {
    object,
    construction,
    sequence: 0n,
    eventId: "01900000-0000-7000-8000-000000000003",
    event: { kind: "alarm", redelivery: null },
    enteredAt: 1000n,
    exitedAt: 1001n,
    units: [],
    sends: [],
    notices: [],
    calls: [],
    alarm: { before: null, after: null, writes: [] },
    batch: null,
    reply: { kind: "alarmDone" },
  },
};
const log = (message: string) => ({ timestamp: 1, level: "info", message });

test("workerd console retains original messages and lossless S1 events", () => {
  const capture = new D4WorkerdConsole();
  const original = [log("unrelated runtime diagnostic"), log(encode(created)), log(encode(event))];
  for (const record of original) capture.observe(record);
  assert.equal(
    capture.transcript(),
    original.map((record): string => `${encode(record)}\n`).join(""),
  );
  assert.deepEqual(capture.records(), [event.centralEvent]);
  assert.deepEqual(capture.constructionRecords(), [created.centralConstruction]);
});

test("workerd console latches malformed S1 without throwing into the runtime callback", () => {
  const capture = new D4WorkerdConsole();
  assert.doesNotThrow(() => capture.observe(log('{"centralEvent":{}}')));
  capture.observe(log(encode(created)));
  assert.throws(() => capture.records(), /console observation failed/);
  assert.throws(() => capture.constructionRecords(), /console observation failed/);
  assert.throws(() => capture.transcript(), /console observation failed/);
});

test("workerd console rejects event without actual construction", () => {
  const capture = new D4WorkerdConsole();
  capture.observe(log(encode(event)));
  assert.throws(() => capture.records(), /console observation failed/);
});

test("workerd console rejects replayed emission sequence", () => {
  const capture = new D4WorkerdConsole();
  capture.observe(log(encode(created)));
  capture.observe(log(encode(event)));
  capture.observe(log(encode(event)));
  assert.throws(() => capture.transcript(), /console observation failed/);
});

test("unrelated diagnostics alone supply no construction proof", () => {
  const capture = new D4WorkerdConsole();
  capture.observe(log("ready"));
  assert.throws(() => capture.transcript(), /no D4 construction observed/);
});
