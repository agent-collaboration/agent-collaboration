import { test } from "node:test";
import assert from "node:assert/strict";
import { encode } from "../../../exact-json.ts";
import {
  D4EventSequence,
  parseD4StderrLine,
  parseD4EventPage,
  requireAlarmCallsFromDueUnits,
  requirePlatformAlarm,
  d4EventRecordSchema,
  type D4EventRecord,
} from "./event-observation.ts";

function identity(index: number): string {
  return `01900000-0000-7000-8000-${index.toString(16).padStart(12, "0")}`;
}
function eventRecord(sequence = 0n, construction = 1, event = 100): D4EventRecord {
  return d4EventRecordSchema.parse({
    object: { kind: "fleet" },
    construction: identity(construction),
    sequence,
    eventId: identity(event),
    event: { kind: "alarm", redelivery: null, delivery: "platform" },
    enteredAt: 1792281600000n,
    exitedAt: 1792281600004n,
    units: [{ unitKind: "summary_call", subject: identity(5), outcome: { kind: "done" } }],
    sends: [],
    notices: [],
    calls: [
      {
        to: { kind: "copy", value: identity(5) },
        callKind: "central.directory-summary",
        unitKind: "summary_call",
        subject: identity(5),
        outcome: "answered",
      },
    ],
    alarm: {
      before: 1792281600000n,
      after: null,
      writes: [{ op: "exact", at: null, unitKind: "recompute" }],
    },
    batch: null,
    reply: { kind: "alarmDone" },
  });
}
function constructionRecord(construction = 1, isolate = 20): string {
  return encode({
    centralConstruction: {
      object: { kind: "fleet" },
      construction: identity(construction),
      cause: construction === 1 ? "start" : "rebuild",
      isolate: identity(isolate),
    },
  });
}
function observeLine(sequence: D4EventSequence, line: string): void {
  sequence.observeLine(line);
}

test("v3.8 Alarm delivery preserves platform and injected labels", (): void => {
  for (const delivery of ["platform", "injected"] as const) {
    const parsed = d4EventRecordSchema.parse({
      ...eventRecord(),
      event: { kind: "alarm", redelivery: null, delivery },
    });
    assert.equal(parsed.event.kind, "alarm");
    assert.ok(parsed.event.kind === "alarm");
    assert.equal(parsed.event.delivery, delivery);
  }
});

test("v3.8 Alarm records without delivery are refused", (): void => {
  assert.throws(() =>
    d4EventRecordSchema.parse({
      ...eventRecord(),
      event: { kind: "alarm", redelivery: null },
    }),
  );
});

test("v3.8 invalid delivery labels and injected redelivery counters are refused", (): void => {
  for (const delivery of ["", "Platform", "clock"]) {
    assert.throws(() =>
      d4EventRecordSchema.parse({
        ...eventRecord(),
        event: { kind: "alarm", redelivery: null, delivery },
      }),
    );
  }
  assert.throws(() =>
    d4EventRecordSchema.parse({
      ...eventRecord(),
      event: { kind: "alarm", redelivery: 0n, delivery: "injected" },
    }),
  );
});

test("v3.8 injected delivery cannot earn platform-wake credit", (): void => {
  assert.doesNotThrow(() => requirePlatformAlarm(eventRecord()));
  const injected = d4EventRecordSchema.parse({
    ...eventRecord(),
    event: { kind: "alarm", redelivery: null, delivery: "injected" },
  });
  assert.throws(() => requirePlatformAlarm(injected), /platform Alarm/);
  const call = d4EventRecordSchema.parse({
    ...eventRecord(),
    event: { kind: "call", callKind: "fixture" },
  });
  assert.throws(() => requirePlatformAlarm(call), /platform Alarm/);
});

test("construction evidence is readable without exposing the sequence's retained state", (): void => {
  const sequence = new D4EventSequence();
  sequence.observeLine(constructionRecord());
  sequence.observeLine(constructionRecord(2));
  const captured = sequence.constructionRecords();
  assert.deepEqual(
    captured.map((record) => [record.construction, record.isolate, record.cause]),
    [
      [identity(1), identity(20), "start"],
      [identity(2), identity(20), "rebuild"],
    ],
  );
  const first = captured[0];
  assert.ok(first);
  first.construction = identity(99);
  assert.equal(sequence.constructionRecords()[0]?.construction, identity(1));
});

test("literal S1 JSON integers remain exact above 2^53 and copy addresses use actual serde shape", (): void => {
  const raw = encode({ centralEvent: { ...eventRecord(), sequence: 9007199254740993n } });
  const parsed = parseD4StderrLine(raw);
  assert.ok(parsed && "centralEvent" in parsed);
  assert.equal(parsed.centralEvent.sequence, 9007199254740993n);
  assert.equal(parsed.centralEvent.calls[0]?.to.kind, "copy");
  assert.equal(parseD4StderrLine("ordinary diagnostic"), undefined);
  assert.throws((): void => {
    parseD4StderrLine('{"centralEvent":');
  });
  assert.throws((): void => {
    parseD4StderrLine('{"centralUnknown":{}}');
  });
});

test("v3.4 retains literal unbound slice-2b fields without granting due-unit or owed bindings", (): void => {
  const unbound = {
    attempt: null,
    attemptKind: null,
    subject: null,
    sendId: null,
    connection: identity(10),
    session: null,
    requestKind: "welcome",
    sentVersion: null,
    batch: null,
    frame: "e30=",
  };
  const input = {
    ...eventRecord(),
    units: [{ unitKind: "handler", subject: null, outcome: { kind: "done" } }],
    calls: [
      {
        to: { kind: "copy", value: identity(5) },
        callKind: "central.directory-summary",
        unitKind: "handler",
        subject: null,
        outcome: "answered",
      },
    ],
    sends: [unbound],
    batch: { claimed: false, startedAt: null, endedAt: null, sources: [] },
  };
  const parsed = d4EventRecordSchema.parse(input);
  assert.equal(parsed.sends[0]?.attempt, null);
  assert.equal(parsed.calls[0]?.subject, null);
  assert.throws(
    (): void => requireAlarmCallsFromDueUnits(parsed, new Set(["handler:null"])),
    /no bound/,
  );
  assert.equal(
    d4EventRecordSchema.parse({ ...input, event: { kind: "call", callKind: null } }).event.kind,
    "call",
  );
  const unknownCall = d4EventRecordSchema.parse({
    ...input,
    reply: { kind: "socketFailed" },
    calls: [{ ...input.calls[0], callKind: null, unitKind: null }],
  });
  assert.equal(unknownCall.reply.kind, "socketFailed");
  assert.equal(unknownCall.calls[0]?.callKind, null);
  assert.throws(
    (): void => requireAlarmCallsFromDueUnits(unknownCall, new Set(["null:null"])),
    /no bound/,
  );
  for (const send of [
    { ...unbound, attempt: identity(99) },
    { ...unbound, session: identity(99) },
    { ...unbound, sendId: identity(99) },
    { ...unbound, subject: undefined },
  ])
    assert.throws((): void => {
      d4EventRecordSchema.parse({ ...input, sends: [send] });
    });
  assert.throws((): void => {
    d4EventRecordSchema.parse({ ...input, batch: { claimed: false } });
  });
  assert.throws((): void => {
    d4EventRecordSchema.parse({
      ...input,
      batch: { claimed: false, startedAt: 1n, endedAt: 2n, sources: [] },
    });
  });
});

test("every construction restarts sequence at zero while an in-instance rebuild retains its separate isolate identity", (): void => {
  const sequence = new D4EventSequence();
  observeLine(sequence, constructionRecord());
  observeLine(sequence, encode({ centralEvent: eventRecord(0n) }));
  observeLine(sequence, encode({ centralEvent: eventRecord(1n, 1, 101) }));
  observeLine(sequence, constructionRecord(2));
  observeLine(sequence, encode({ centralEvent: eventRecord(0n, 2, 102) }));
  sequence.requireComplete();
  assert.equal(sequence.records().length, 3);
});

test("a gap, duplicate UUID, unknown construction or repeated construction leaves capture invalid", (): void => {
  for (const wrong of [
    encode({ centralEvent: eventRecord(2n) }),
    encode({ centralEvent: eventRecord(1n, 1, 100) }),
    encode({ centralEvent: eventRecord(0n, 2, 102) }),
    constructionRecord(),
  ]) {
    const sequence = new D4EventSequence();
    observeLine(sequence, constructionRecord());
    observeLine(sequence, encode({ centralEvent: eventRecord() }));
    assert.throws((): void => observeLine(sequence, wrong));
    assert.throws((): void => sequence.requireComplete(), /sequence errors/);
  }
});

test("lossless send bindings require UUIDv7, exact base64, valid batch index and status-only version", (): void => {
  const send = {
    attempt: identity(10),
    attemptKind: "status",
    subject: identity(11),
    sendId: identity(12),
    connection: identity(13),
    session: identity(14),
    requestKind: "publishStatus",
    sentVersion: 9007199254740993n,
    batch: { batchSendId: identity(15), index: 0n, items: 32n },
    frame: Buffer.from("original payload").toString("base64"),
  };
  assert.equal(
    d4EventRecordSchema.parse({ ...eventRecord(), sends: [send] }).sends[0]?.sentVersion,
    9007199254740993n,
  );
  for (const change of [
    { attempt: "not-v7" },
    { frame: "???" },
    { sentVersion: null },
    { attemptKind: "push" },
    { batch: { batchSendId: identity(15), index: 32n, items: 32n } },
  ])
    assert.throws((): void => {
      d4EventRecordSchema.parse({ ...eventRecord(), sends: [{ ...send, ...change }] });
    });
});

test("ring cursor remains monotonic across per-construction sequence resets and rejects truncated or missing ranges", (): void => {
  const address = { kind: "fleet" } as const;
  const response = {
    kind: "events",
    value: {
      firstRetained: 1n,
      through: 2n,
      records: [
        { cursor: 1n, record: eventRecord() },
        { cursor: 2n, record: eventRecord(0n, 2, 101) },
      ],
    },
  };
  assert.equal(parseD4EventPage({ address, after: 0n, response }).through, 2n);
  for (const value of [
    { ...response.value, firstRetained: 2n },
    { ...response.value, records: response.value.records.slice(1) },
    { ...response.value, records: response.value.records.slice(0, 1) },
    {
      ...response.value,
      records: [
        { cursor: 1n, record: { ...eventRecord(), object: { kind: "copy", value: identity(4) } } },
        response.value.records[1],
      ],
    },
  ])
    assert.throws((): void => {
      parseD4EventPage({ address, after: 0n, response: { kind: "events", value } });
    });
});

test("an Alarm call must carry its expected due unit identity, not just occur in the same event", (): void => {
  const record = eventRecord();
  requireAlarmCallsFromDueUnits(record, new Set([`summary_call:${identity(5)}`]));
  assert.throws(
    (): void => requireAlarmCallsFromDueUnits(record, new Set([`summary_call:${identity(6)}`])),
    /not bound/,
  );
  assert.throws(
    (): void =>
      requireAlarmCallsFromDueUnits(
        { ...record, units: [] },
        new Set([`summary_call:${identity(5)}`]),
      ),
    /not bound/,
  );
});

test("malformed named records stay fatal after a caller catches them, and returned records cannot rewrite capture", (): void => {
  const sequence = new D4EventSequence();
  observeLine(sequence, constructionRecord());
  const record = eventRecord();
  sequence.observe({ centralEvent: record });
  record.calls.length = 0;
  const view = sequence.records();
  const first = view[0];
  assert.ok(first);
  first.calls.length = 0;
  assert.equal(sequence.records()[0]?.calls.length, 1);
  assert.throws((): void => sequence.observeLine('{"centralEvent":'));
  assert.throws((): void => sequence.requireComplete(), /sequence errors/);
});
