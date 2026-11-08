import { test } from "node:test";
import assert from "node:assert/strict";
import { D4ControlClient } from "./control-client.ts";
import { fixture, type ControlFixture } from "./control-fixture.ts";
import { DrainedHourFailure, observeTokioDrainedHour } from "./drained-hour.ts";

const from = "2026-11-08T00:00:00Z";
const through = "2026-11-08T01:00:00Z";
const emptySnapshot = {
  kind: "snapshot",
  value: {
    attempts: [],
    sessions: [],
    inventory: [],
    windows: [],
    noticeProgress: [],
    feed: { head: 0n, retainedThrough: 0n, effectiveFloor: 0n },
    owners: { outbox: [], statusOwed: [] },
  },
};
const emptyRing = { kind: "events", value: { records: [], firstRetained: 1n, through: 0n } };
const pending = { kind: "pending", value: { tasks: 0n, deadlines: 0n, waiters: 0n } };
const alarm = { kind: "alarm", value: { stored: null } };
const settled = { kind: "clockSettled", value: { alarmEvents: 0n } };
function answers(): unknown[] {
  return [
    settled,
    alarm,
    pending,
    emptySnapshot,
    emptyRing,
    settled,
    emptyRing,
    alarm,
    pending,
    emptySnapshot,
  ];
}
function client(input: ControlFixture, shell: "tokio" | "workerd" = "tokio"): D4ControlClient {
  return new D4ControlClient({
    owner: input.owner,
    address: { kind: "fleet" },
    shell,
    passivity: "observed",
    clockPassivity: "observed",
  });
}
function ring(kind: "alarm" | "frame"): unknown {
  return {
    kind: "events",
    value: {
      firstRetained: 1n,
      through: 1n,
      records: [
        {
          cursor: 1n,
          record: {
            object: { kind: "fleet" },
            construction: "01900000-0000-7000-8000-000000000001",
            sequence: 0n,
            eventId: "01900000-0000-7000-8000-000000000002",
            event:
              kind === "alarm"
                ? { kind: "alarm", redelivery: null }
                : {
                    kind: "frame",
                    connection: "01900000-0000-7000-8000-000000000003",
                    frame: "e30=",
                  },
            enteredAt: 1794099600000n,
            exitedAt: 1794099600001n,
            units: [],
            sends: [],
            notices: [],
            calls: [],
            alarm: { before: null, after: null, writes: [] },
            batch: null,
            reply: { kind: kind === "alarm" ? "alarmDone" : "none" },
          },
        },
      ],
    },
  };
}
function failedCheck(check: string): (error: unknown) => boolean {
  return (error): boolean => error instanceof DrainedHourFailure && error.check === check;
}

test("J4 controller crosses the owner HTTP route and reads only quiet interval boundaries after settled epochs", async (): Promise<void> => {
  const input = await fixture(answers());
  try {
    const result = await observeTokioDrainedHour({ controls: client(input), from, through });
    assert.equal(result.alarmEvents, 0n);
    assert.equal(result.runtimeQualified, false);
    assert.ok(
      result.remainingQualification.some((gap): boolean => gap.includes("all non-attempt")),
    );
    assert.deepEqual(
      input.requests.map((request) => (request as { command: { kind: string } }).command.kind),
      [
        "settleClock",
        "readAlarm",
        "readPending",
        "snapshot",
        "readEvents",
        "settleClock",
        "readEvents",
        "readAlarm",
        "readPending",
        "snapshot",
      ],
    );
    assert.deepEqual(input.requests[5], {
      address: { kind: "fleet" },
      command: { kind: "settleClock", value: { now: through } },
    });
  } finally {
    await input.close();
  }
});

test("a forced or dropped Alarm count fails J4 even when the ring and alarm look empty", async (): Promise<void> => {
  const responses = answers();
  responses[5] = { kind: "clockSettled", value: { alarmEvents: 1n } };
  const input = await fixture(responses);
  try {
    await assert.rejects(
      observeTokioDrainedHour({ controls: client(input), from, through }),
      failedCheck("zero-alarm-invocations"),
    );
  } finally {
    await input.close();
  }
});

test("an actual recorded Alarm cannot hide behind a zero settled count", async (): Promise<void> => {
  const responses = answers();
  responses[6] = ring("alarm");
  const input = await fixture(responses);
  try {
    await assert.rejects(
      observeTokioDrainedHour({ controls: client(input), from, through }),
      failedCheck("zero-alarm-invocations"),
    );
  } finally {
    await input.close();
  }
});

test("application traffic and truncated event coverage cannot qualify a quiet window", async (): Promise<void> => {
  for (const delta of [
    ring("frame"),
    { kind: "events", value: { records: [], firstRetained: 5n, through: 4n } },
  ]) {
    const responses = answers();
    responses[6] = delta;
    const input = await fixture(responses);
    try {
      await assert.rejects(observeTokioDrainedHour({ controls: client(input), from, through }));
    } finally {
      await input.close();
    }
  }
});

test("an undrained boundary stops before the measured clock move", async (): Promise<void> => {
  for (const [index, value, check] of [
    [1, { kind: "alarm", value: { stored: 1794099600000n } }, "no-stored-alarm"],
    [2, { kind: "pending", value: { tasks: 1n, deadlines: 0n, waiters: 0n } }, "work-drained"],
  ] as const) {
    const responses = answers();
    responses[index] = value;
    const input = await fixture(responses);
    try {
      await assert.rejects(
        observeTokioDrainedHour({ controls: client(input), from, through }),
        failedCheck(check),
      );
      assert.equal(input.requests.length, 4);
    } finally {
      await input.close();
    }
  }
});

test("legacy done and a short interval cannot be relabeled a settled hour", async (): Promise<void> => {
  const responses = answers();
  responses[5] = { kind: "done" };
  const input = await fixture(responses);
  try {
    await assert.rejects(
      observeTokioDrainedHour({ controls: client(input), from, through: "2026-11-08T00:01:00Z" }),
      failedCheck("one-hour"),
    );
    assert.equal(input.requests.length, 0);
    await assert.rejects(observeTokioDrainedHour({ controls: client(input), from, through }));
    assert.equal(input.requests.length, 6);
  } finally {
    await input.close();
  }
});

test("an injected Tokio hour can never replace workerd's real hour", async (): Promise<void> => {
  const input = await fixture([]);
  try {
    await assert.rejects(
      observeTokioDrainedHour({ controls: client(input, "workerd"), from, through }),
      /Tokio-only/,
    );
    assert.equal(input.requests.length, 0);
  } finally {
    await input.close();
  }
});
