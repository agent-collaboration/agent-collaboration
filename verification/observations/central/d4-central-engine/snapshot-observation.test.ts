import { test } from "node:test";
import assert from "node:assert/strict";
import { decode, encode } from "../../../exact-json.ts";
import {
  d4SnapshotResponseSchema,
  requireWindowSnapshotAgreement,
  d4LongOperationsResponseSchema,
  requireCentralLongOperations,
  parseD4Snapshot,
  type D4Snapshot,
} from "./snapshot-observation.ts";

function identity(index: number): string {
  return `01900000-0000-7000-8000-${index.toString(16).padStart(12, "0")}`;
}
function attempt(): D4Snapshot["attempts"][number] {
  return {
    id: identity(1),
    subject: identity(2),
    kind: "status",
    state: "in_flight",
    sendId: identity(3),
    connectionId: identity(4),
    sessionId: identity(5),
    sentVersion: 9007199254740993n,
    nextAttemptAt: 1792281630000n,
    attempts: 1n,
    firstFailedAt: null,
    lastErrorClass: null,
    lastError: null,
    createdAt: 1792281600000n,
    updatedAt: 1792281600000n,
  };
}
function snapshot(): D4Snapshot {
  return {
    attempts: [attempt()],
    sessions: [
      {
        session: identity(5),
        peer: identity(6),
        connection: identity(4),
        registration: "attached",
      },
    ],
    inventory: [identity(4)],
    windows: [{ connection: identity(4), inFlight: 1n }],
    noticeProgress: [
      { machine: identity(6), lastIssued: 45n, expiredThrough: 40n, appliedThrough: null },
    ],
    feed: { head: 120n, retainedThrough: 98n, effectiveFloor: 110n },
    owners: {
      outbox: [{ item: identity(2), custody: "relay_held" }],
      statusOwed: [{ id: identity(2), item: identity(7), version: 9007199254740993n }],
    },
  };
}
function parse(value: unknown): D4Snapshot {
  return d4SnapshotResponseSchema.parse(decode(encode({ kind: "snapshot", value }))).value;
}

test("literal snake_case attempt states, explicit null and exact versions survive wire decoding", (): void => {
  const value = snapshot();
  value.attempts.push({
    ...attempt(),
    id: identity(8),
    subject: identity(9),
    kind: "push",
    state: "waiting",
    sendId: null,
    connectionId: null,
    sessionId: null,
    sentVersion: null,
    nextAttemptAt: null,
  });
  value.attempts.push({
    ...attempt(),
    id: identity(10),
    subject: identity(11),
    kind: "notice_expiry",
    state: "given_up",
    sendId: null,
    connectionId: null,
    sessionId: null,
    sentVersion: null,
    nextAttemptAt: null,
    firstFailedAt: 1792195200000n,
    lastErrorClass: "corrupt_row",
    lastError: "corrupt notice",
  });
  const parsed = parse(value);
  assert.equal(parsed.attempts[0]?.sentVersion, 9007199254740993n);
  assert.equal(parsed.noticeProgress[0]?.appliedThrough, null);
  requireWindowSnapshotAgreement(parsed, new Map([[identity(4), 1n]]));
});

test("missing/invalid stored state, in-flight binding or retired deadline is never coerced", (): void => {
  for (const change of [
    { state: "unknown" },
    { state: "inFlight" },
    { sendId: null },
    { sessionId: null },
    { sentVersion: null },
    { state: "given_up" },
    { id: "bad-id" },
  ])
    assert.throws((): void => {
      parse({ ...snapshot(), attempts: [{ ...attempt(), ...change }] });
    });
  const { connectionId: _missing, ...incomplete } = attempt();
  assert.throws((): void => {
    parse({ ...snapshot(), attempts: [incomplete] });
  });
});

test("duplicate rows and current socket aliases cannot silently shrink or inflate the window", (): void => {
  const value = snapshot();
  assert.throws((): void => {
    parse({ ...value, attempts: [attempt(), { ...attempt(), id: identity(8) }] });
  });
  assert.throws((): void => {
    parse({ ...value, inventory: [identity(4), identity(4)] });
  });
  assert.throws((): void => {
    parse({
      ...value,
      sessions: [...value.sessions, { ...value.sessions[0], session: identity(20) }],
    });
  });
});

test("reported counts must agree with real current-session rows and observed wire slots at a causal barrier", (): void => {
  const value = snapshot();
  requireWindowSnapshotAgreement(parse(value), new Map([[identity(4), 1n]]));
  assert.throws((): void => requireWindowSnapshotAgreement(parse(value), new Map()), /mismatch/);
  assert.throws(
    (): void =>
      requireWindowSnapshotAgreement(
        parse({ ...value, windows: [{ connection: identity(4), inFlight: 0n }] }),
        new Map([[identity(4), 1n]]),
      ),
    /mismatch/,
  );
  const stale = {
    ...attempt(),
    id: identity(8),
    subject: identity(9),
    connectionId: identity(14),
    sessionId: identity(15),
  };
  requireWindowSnapshotAgreement(
    parse({ ...value, attempts: [attempt(), stale] }),
    new Map([[identity(4), 1n]]),
  );
  const ended = parse({
    ...value,
    inventory: [],
    windows: [{ connection: identity(4), inFlight: 0n }],
  });
  requireWindowSnapshotAgreement(ended, new Map());
});

test("33 actual current attempts fail even when a reported count tries to hide them", (): void => {
  const value = snapshot();
  const attempts = Array.from({ length: 33 }, (_, index) => ({
    ...attempt(),
    id: identity(100 + index),
    subject: identity(200 + index),
    sendId: identity(300 + index),
  }));
  assert.throws(
    (): void =>
      requireWindowSnapshotAgreement(
        parse({ ...value, attempts, windows: [{ connection: identity(4), inFlight: 32n }] }),
        new Map([[identity(4), 32n]]),
      ),
    /exceeds 32/,
  );
});

test("exact central U6 registration and real driveOne evidence reject extra kinds or a stand-in", (): void => {
  const value = {
    registeredKinds: [
      "fleet-sync.register",
      "fleet-sync.import",
      "fleet-sync.retire",
      "fleet-sync.holder-change",
      "fleet-sync.holder-change-cancel",
    ],
    drives: [{ operation: identity(1), by: "driveOne", eventId: identity(2) }],
    storedHolderReplies: [
      {
        call: { operation: identity(1), step: "release" },
        target: "fleet-sync.holder-release",
        stored: true,
      },
    ],
  };
  const parseOperations = (
    value: unknown,
  ): ReturnType<typeof d4LongOperationsResponseSchema.parse>["value"] =>
    d4LongOperationsResponseSchema.parse(decode(encode({ kind: "longOperations", value }))).value;
  requireCentralLongOperations(parseOperations(value), [identity(1)]);
  assert.throws(
    (): void =>
      requireCentralLongOperations(
        parseOperations({
          ...value,
          registeredKinds: [...value.registeredKinds, "fleet-sync.delivery.push"],
        }),
        [identity(1)],
      ),
    /five/,
  );
  assert.throws(
    (): void =>
      requireCentralLongOperations(
        parseOperations({ ...value, drives: [{ ...value.drives[0], by: "standIn" }] }),
        [identity(1)],
      ),
    /actual drive_one/,
  );
  assert.throws(
    (): void => requireCentralLongOperations(parseOperations(value), [identity(3)]),
    /actual drive_one/,
  );
});

test("Copy has explicit null/empty Fleet fields while a Fleet snapshot must carry its feed", (): void => {
  const copy = {
    attempts: [],
    sessions: [],
    inventory: [],
    windows: [],
    noticeProgress: [],
    feed: null,
    owners: { outbox: [], statusOwed: [] },
  };
  assert.equal(
    parseD4Snapshot({ kind: "copy", value: identity(5) }, { kind: "snapshot", value: copy }).feed,
    null,
  );
  assert.throws((): void => {
    parseD4Snapshot({ kind: "fleet" }, { kind: "snapshot", value: copy });
  }, /lacks its feed/);
  assert.throws((): void => {
    parseD4Snapshot({ kind: "copy", value: identity(5) }, { kind: "snapshot", value: snapshot() });
  }, /Fleet-only/);
});
