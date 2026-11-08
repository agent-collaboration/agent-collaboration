import { test } from "node:test";
import assert from "node:assert/strict";
import { z } from "zod";
import { fixture } from "./control-fixture.ts";
import { D4ControlClient } from "./control-client.ts";
import { d4EventRecordSchema, type D4EventRecord } from "./event-observation.ts";
import { driveStaleAlarmRepair, StaleAlarmRepairFailure } from "./stale-alarm.ts";

const construction = "01900000-0000-7000-8000-000000000001";
const eventId = "01900000-0000-7000-8000-000000000002";
const connection = "01900000-0000-7000-8000-000000000003";
const businessTime = 2000000050000n;
const staleAt = 2000000000000n;
const payload = Buffer.from('{"kind":"hello","value":{}}');

function completed(): D4EventRecord {
  return d4EventRecordSchema.parse({
    object: { kind: "fleet" },
    construction,
    sequence: 0n,
    eventId,
    event: { kind: "frame", connection, frame: payload.toString("base64") },
    enteredAt: businessTime,
    exitedAt: businessTime,
    units: [{ unitKind: "handler", subject: null, outcome: { kind: "done" } }],
    sends: [],
    notices: [],
    calls: [],
    alarm: {
      before: staleAt,
      after: null,
      writes: [{ op: "exact", at: null, unitKind: "repair" }],
    },
    batch: { claimed: true, startedAt: businessTime, endedAt: businessTime, sources: [] },
    reply: { kind: "none" },
  });
}

async function apparatus(
  input: {
    record?: D4EventRecord;
    seedEntersKernel?: boolean;
    seededReadback?: bigint | null;
    finalReadback?: bigint | null;
    shell?: "tokio" | "workerd";
  } = {},
) {
  const records: D4EventRecord[] = [];
  let reads = 0;
  let sends = 0;
  const http = await fixture((raw) => {
    const command = z.object({ command: z.object({ kind: z.string() }) }).parse(raw).command;
    if (command.kind === "seed") {
      if (input.seedEntersKernel) records.push(completed());
      return { kind: "done" };
    }
    assert.equal(command.kind, "readAlarm");
    reads += 1;
    return {
      kind: "alarm",
      value: {
        stored:
          reads === 1
            ? input.seededReadback === undefined
              ? staleAt
              : input.seededReadback
            : (input.finalReadback ?? null),
      },
    };
  });
  const controls = new D4ControlClient({
    owner: http.owner,
    address: { kind: "fleet" },
    shell: input.shell ?? "tokio",
    passivity: "observed",
    clockPassivity: "unqualified",
  });
  return {
    http,
    sends: (): number => sends,
    drive: () =>
      driveStaleAlarmRepair({
        controls,
        shell: input.shell ?? "tokio",
        businessTime,
        staleAt,
        events: () => records,
        frame: {
          connection,
          bytes: payload,
          async send(bytes): Promise<void> {
            assert.deepEqual(bytes, payload);
            sends += 1;
            records.push(input.record ?? completed());
          },
        },
      }),
  };
}

for (const shell of ["tokio", "workerd"] as const)
  test(`stale controller retains original Frame and scoped evidence on ${shell}`, async () => {
    const target = await apparatus({ shell });
    try {
      const result = await target.drive();
      assert.equal(result.runtimeQualified, false);
      assert.equal(result.subcase, "stale-alarm-repair-on-Frame");
      assert.deepEqual(result.record, completed());
      assert.equal(result.storedAfter, null);
      assert.equal(target.sends(), 1);
      assert.equal(target.http.requests.length, 3);
    } finally {
      await target.http.close();
    }
  });

for (const scenario of [
  {
    name: "missing repair",
    check: "repair-before-handler",
    mutate: (record: D4EventRecord) => {
      record.alarm.writes = [{ op: "exact", at: null, unitKind: "recompute" }];
    },
  },
  {
    name: "handler write before repair",
    check: "repair-before-handler",
    mutate: (record: D4EventRecord) => {
      record.alarm.writes.unshift({ op: "lower", at: businessTime, unitKind: "handler" });
    },
  },
  {
    name: "unfinished handler",
    check: "handler-completed",
    mutate: (record: D4EventRecord) => {
      record.units = [];
    },
  },
  {
    name: "mixed handler outcomes",
    check: "handler-completed",
    mutate: (record: D4EventRecord) => {
      record.units.push({
        unitKind: "handler",
        subject: null,
        outcome: { kind: "failed", errorClass: "fixture", message: "handler failed" },
      });
    },
  },
  {
    name: "declined batch",
    check: "frame-claims-due-batch",
    mutate: (record: D4EventRecord) => {
      record.batch = { claimed: false, startedAt: null, endedAt: null, sources: [] };
    },
  },
  {
    name: "wrong business clock",
    check: "business-time",
    mutate: (record: D4EventRecord) => {
      record.enteredAt += 1n;
    },
  },
  {
    name: "stale alarm consumed before Frame",
    check: "stored-stale-alarm",
    mutate: (record: D4EventRecord) => {
      record.alarm.before = null;
    },
  },
] as const)
  test(`stale controller rejects ${scenario.name} without replaying the Frame`, async () => {
    const record = completed();
    scenario.mutate(record);
    const target = await apparatus({ record });
    try {
      await assert.rejects(
        target.drive(),
        (error: unknown): boolean =>
          error instanceof StaleAlarmRepairFailure && error.check === scenario.check,
      );
      assert.equal(target.sends(), 1);
    } finally {
      await target.http.close();
    }
  });

for (const scenario of [
  { input: { seedEntersKernel: true }, check: "passive-fixture" },
  { input: { seededReadback: null }, check: "stored-stale-alarm" },
] as const)
  test(`stale controller refuses ${scenario.check} setup before sending`, async () => {
    const target = await apparatus(scenario.input);
    try {
      await assert.rejects(
        target.drive(),
        (error: unknown): boolean =>
          error instanceof StaleAlarmRepairFailure && error.check === scenario.check,
      );
      assert.equal(target.sends(), 0);
    } finally {
      await target.http.close();
    }
  });

test("stale controller rejects final platform/S1 disagreement", async () => {
  const target = await apparatus({ finalReadback: businessTime + 1000n });
  try {
    await assert.rejects(
      target.drive(),
      (error: unknown): boolean =>
        error instanceof StaleAlarmRepairFailure && error.check === "deadline-replaced",
    );
    assert.equal(target.sends(), 1);
  } finally {
    await target.http.close();
  }
});
