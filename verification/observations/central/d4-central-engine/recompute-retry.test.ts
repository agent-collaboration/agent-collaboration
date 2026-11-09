import { test } from "node:test";
import assert from "node:assert/strict";
import { z } from "zod";
import { fixture } from "./control-fixture.ts";
import { D4ControlClient } from "./control-client.ts";
import { d4EventRecordSchema, type D4EventRecord } from "./event-observation.ts";
import { driveTokioRecomputeRetry, RecomputeRetryFailure } from "./recompute-retry.ts";

const clock = 1794153600000n;
const retryAt = clock + 1000n;
const faultId = "01900000-0000-7000-8000-000000000010";
type Fault =
  | "none"
  | "unreached"
  | "done-on-failure"
  | "missing-deadline"
  | "extra-retry"
  | "retained-deadline"
  | "arm-enters-kernel"
  | "no-auto-retry"
  | "rearmed"
  | "dirty-fixture";

function alarm(index: number, fault: Fault): D4EventRecord {
  return d4EventRecordSchema.parse({
    object: { kind: "fleet" },
    construction: "01900000-0000-7000-8000-000000000001",
    sequence: BigInt(index),
    eventId: `01900000-0000-7000-8000-${(index + 20).toString(16).padStart(12, "0")}`,
    event: { kind: "alarm", redelivery: BigInt(index), delivery: "platform" },
    enteredAt: index === 0 ? clock : retryAt + BigInt(index),
    exitedAt: index === 0 ? clock : retryAt + BigInt(index),
    units: [],
    sends: [],
    notices: [],
    calls: [],
    alarm: { before: null, after: null, writes: [] },
    batch: null,
    reply: { kind: index === 0 && fault !== "done-on-failure" ? "alarmRetry" : "alarmDone" },
  });
}

async function apparatus(fault: Fault = "none") {
  let phase = 0;
  let armed = false;
  let stimuli = 0;
  let arms = 0;
  let drains = 0;
  const http = await fixture((raw) => {
    const command = z
      .object({ command: z.object({ kind: z.string(), value: z.unknown().optional() }) })
      .parse(raw).command;
    if (command.kind === "readPending")
      return { kind: "pending", value: { tasks: 0n, deadlines: 0n, waiters: 0n } };
    if (command.kind === "readAlarm")
      return {
        kind: "alarm",
        value: {
          stored:
            phase === 1
              ? fault === "missing-deadline"
                ? null
                : retryAt
              : fault === "dirty-fixture" || (phase === 2 && fault === "retained-deadline")
                ? clock + 30000n
                : null,
        },
      };
    if (command.kind === "armFault") {
      assert.deepEqual(command.value, {
        fault: { kind: "exactAlarmWrite", value: { on: "alarm" } },
        count: 1n,
      });
      armed = true;
      arms += 1;
      return { kind: "faultArmed", value: { faultId } };
    }
    if (command.kind === "readEvents") {
      let records =
        phase === 0 ? [] : phase === 1 ? [alarm(0, fault)] : [alarm(0, fault), alarm(1, fault)];
      if (phase === 2 && fault === "extra-retry") records.push(alarm(2, fault));
      if (phase === 0 && armed && fault === "arm-enters-kernel") records = [alarm(0, fault)];
      return {
        kind: "events",
        value: {
          records: records.map((record, index) => ({ cursor: BigInt(index + 1), record })),
          firstRetained: 1n,
          through: BigInt(records.length),
        },
      };
    }
    if (command.kind === "duplicateAlarm") {
      assert.equal(armed, true);
      phase = 1;
      stimuli += 1;
      return { kind: "done" };
    }
    if (command.kind === "readFault") {
      assert.deepEqual(command.value, { faultId });
      return {
        kind: "fault",
        value:
          phase === 1
            ? {
                armed: 1n,
                reached: fault === "unreached" ? 0n : 1n,
                consumed: fault === "unreached" ? 0n : 1n,
              }
            : { armed: fault === "rearmed" ? 2n : 1n, reached: 2n, consumed: 1n },
      };
    }
    assert.equal(command.kind, "settleClock");
    assert.deepEqual(command.value, { now: new Date(Number(retryAt + 1n)).toISOString() });
    drains += 1;
    phase = fault === "no-auto-retry" ? 1 : 2;
    return {
      kind: "clockSettled",
      value: { alarmEvents: fault === "no-auto-retry" ? 0n : fault === "extra-retry" ? 2n : 1n },
    };
  });
  const controls = new D4ControlClient({
    owner: http.owner,
    address: { kind: "fleet" },
    shell: "tokio",
    passivity: "observed",
    clockPassivity: "observed",
  });
  return {
    http,
    counts: () => ({ arms, stimuli, drains }),
    drive: () => driveTokioRecomputeRetry({ controls }),
  };
}

test("recompute controller drives one stimulus then one causal automatic retry", async () => {
  const target = await apparatus();
  try {
    const result = await target.drive();
    assert.equal(result.runtimeQualified, false);
    assert.deepEqual(
      result.records.map((record): string => record.reply.kind),
      ["alarmRetry", "alarmDone"],
    );
    assert.deepEqual(result.fault, { armed: 1n, reached: 2n, consumed: 1n });
    assert.equal(result.storedAfter, null);
    assert.deepEqual(target.counts(), { arms: 1, stimuli: 1, drains: 1 });
  } finally {
    await target.http.close();
  }
});

for (const scenario of [
  { fault: "unreached", check: "one-shot-fault-reached" },
  { fault: "done-on-failure", check: "failed-alarm-requests-retry" },
  { fault: "missing-deadline", check: "platform-retry-deadline" },
  { fault: "extra-retry", check: "one-automatic-retry" },
  { fault: "retained-deadline", check: "quiet-after-recovery" },
  { fault: "arm-enters-kernel", check: "passive-fault-control" },
  { fault: "no-auto-retry", check: "one-automatic-retry" },
  { fault: "rearmed", check: "one-shot-fault-consumed" },
  { fault: "dirty-fixture", check: "quiet-fixture" },
] as const)
  test(`recompute controller rejects ${scenario.fault} without replay`, async () => {
    const target = await apparatus(scenario.fault);
    try {
      await assert.rejects(
        target.drive(),
        (error: unknown): boolean =>
          error instanceof RecomputeRetryFailure && error.check === scenario.check,
      );
      const counts = target.counts();
      assert.ok(counts.arms <= 1 && counts.stimuli <= 1 && counts.drains <= 1);
      if (scenario.fault === "dirty-fixture")
        assert.deepEqual(counts, { arms: 0, stimuli: 0, drains: 0 });
    } finally {
      await target.http.close();
    }
  });
