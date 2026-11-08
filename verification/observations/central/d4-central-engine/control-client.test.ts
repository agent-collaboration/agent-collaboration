import { fixture, type ControlFixture } from "./control-fixture.ts";
import { test } from "node:test";
import assert from "node:assert/strict";
import { D4ControlClient } from "./control-client.ts";

const subject = "01900000-0000-7000-8000-000000000001";
function client(
  input: ControlFixture,
  changes: Partial<ConstructorParameters<typeof D4ControlClient>[0]> = {},
): D4ControlClient {
  return new D4ControlClient({
    owner: input.owner,
    address: { kind: "fleet" },
    shell: "tokio",
    passivity: "observed",
    clockPassivity: "observed",
    ...changes,
  });
}

test("literal draft control requests cross the real owner HTTP serializer and preserve exact integer replies", async (): Promise<void> => {
  const input = await fixture([
    { kind: "alarm", value: { stored: 1792281630000n } },
    { kind: "faultArmed", value: { faultId: subject } },
    { kind: "fault", value: { armed: 9007199254740993n, reached: 1n, consumed: 1n } },
  ]);
  try {
    const controls = client(input, { address: { kind: "copy", value: subject } });
    assert.equal(await controls.readAlarm(), 1792281630000n);
    assert.equal(
      await controls.armFault({ kind: "exactAlarmWrite", value: { on: "frame" } }, 1n),
      subject,
    );
    assert.equal((await controls.readFault(subject)).armed, 9007199254740993n);
    assert.deepEqual(input.requests, [
      { address: { kind: "copy", value: subject }, command: { kind: "readAlarm" } },
      {
        address: { kind: "copy", value: subject },
        command: {
          kind: "armFault",
          value: { fault: { kind: "exactAlarmWrite", value: { on: "frame" } }, count: 1n },
        },
      },
      {
        address: { kind: "copy", value: subject },
        command: { kind: "readFault", value: { faultId: subject } },
      },
    ]);
  } finally {
    await input.close();
  }
});

test("unqualified controls and the old clock wake path refuse before any request", async (): Promise<void> => {
  const input = await fixture([]);
  try {
    await assert.rejects(
      client(input, { passivity: "unqualified" }).readAlarm(),
      /passivity is unqualified/,
    );
    await assert.rejects(
      client(input, { clockPassivity: "unqualified" }).settleClock("2026-11-08T00:00:00Z"),
      /drain\/passivity is unqualified/,
    );
    assert.deepEqual(input.requests, []);
  } finally {
    await input.close();
  }
});

test("SettleClock requires the settled clock epoch reply and preserves its exact Alarm count", async (): Promise<void> => {
  const input = await fixture([
    { kind: "clockSettled", value: { alarmEvents: 9007199254740993n } },
    { kind: "done" },
  ]);
  try {
    assert.equal(await client(input).settleClock("2026-11-08T00:00:00Z"), 9007199254740993n);
    await assert.rejects(client(input).settleClock("2026-11-08T01:00:00Z"));
    assert.deepEqual(input.requests[0], {
      address: { kind: "fleet" },
      command: { kind: "settleClock", value: { now: "2026-11-08T00:00:00Z" } },
    });
  } finally {
    await input.close();
  }
});

test("Tokio-only controls never leak onto workerd", async (): Promise<void> => {
  const input = await fixture([]);
  try {
    const controls = client(input, { shell: "workerd" });
    for (const call of [
      (): Promise<unknown> => controls.readPending(),
      (): Promise<unknown> => controls.readEvents(0n),
      (): Promise<unknown> => controls.rebuildAtQuiescence(true),
      (): Promise<unknown> => controls.settleClock("2026-11-08T00:00:00Z"),
      (): Promise<unknown> => controls.barrier("beforeClaim", null),
    ])
      await assert.rejects(call, /Tokio-only/);
    assert.deepEqual(input.requests, []);
  } finally {
    await input.close();
  }
});

test("beforeClaim preserves the atomic claim boundary and replies use exact adjacent tags", async (): Promise<void> => {
  const input = await fixture([
    { kind: "barrierArmed", value: { barrierId: subject } },
    { kind: "barrier", value: { reached: true, eventId: subject } },
    { kind: "done" },
  ]);
  try {
    const controls = client(input);
    await controls.barrier("beforeClaim", null);
    assert.equal(await controls.awaitBarrier(subject), subject);
    await controls.releaseBarrier(subject);
    assert.deepEqual(input.requests[0], {
      address: { kind: "fleet" },
      command: { kind: "barrier", value: { point: "beforeClaim", unitKind: null } },
    });
  } finally {
    await input.close();
  }
});

test("the ring wrapper rejects truncation rather than relabeling it an empty quiet interval", async (): Promise<void> => {
  const input = await fixture([
    { kind: "events", value: { records: [], firstRetained: 4n, through: 3n } },
  ]);
  try {
    await assert.rejects(client(input).readEvents(0n), /truncated/);
  } finally {
    await input.close();
  }
});

test("wrong response tags and invalid fault data fail without implicit retries", async (): Promise<void> => {
  const input = await fixture([{ kind: "done" }]);
  try {
    await assert.rejects(client(input).readAlarm());
    await assert.rejects(client(input).armFault({ kind: "attemptsWrite" }, 0n));
    assert.equal(input.requests.length, 1);
  } finally {
    await input.close();
  }
});

test("fixture seed data has the exact nested wire shape and preserves integer notice positions", async (): Promise<void> => {
  const input = await fixture([{ kind: "done" }]);
  try {
    await client(input).seedFixture({
      kind: "noticeLog",
      value: {
        machine: subject,
        clearedThrough: 98n,
        positions: [99n, 100n],
        expiresAt: 1792281600000n,
      },
    });
    assert.deepEqual(input.requests[0], {
      address: { kind: "fleet" },
      command: {
        kind: "seed",
        value: {
          seed: {
            kind: "noticeLog",
            value: {
              machine: subject,
              clearedThrough: 98n,
              positions: [99n, 100n],
              expiresAt: 1792281600000n,
            },
          },
        },
      },
    });
  } finally {
    await input.close();
  }
});
