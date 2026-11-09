import { fixture, type ControlFixture } from "./control-fixture.ts";
import { test, mock } from "node:test";
import assert from "node:assert/strict";
import { D4ControlClient, D4ControlFailure } from "./control-client.ts";

const subject = "01900000-0000-7000-8000-000000000001";
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

test("v3.6 retries only a confirmed unstable read and retains the stable snapshot", async (): Promise<void> => {
  const input = await fixture([new Response('"snapshotUnstable"', { status: 409 }), emptySnapshot]);
  try {
    assert.equal((await client(input).snapshot()).feed?.head, 0n);
    assert.equal(input.requests.length, 2);
  } finally {
    await input.close();
  }
});

test("v3.6 snapshot retry is bounded and notOpen or corrupt refusals remain immediate", async (): Promise<void> => {
  const input = await fixture(() => new Response('"snapshotUnstable"', { status: 409 }));
  try {
    await assert.rejects(
      client(input).snapshot(),
      (error: unknown): boolean =>
        error instanceof D4ControlFailure && error.failure === "snapshotUnstable",
    );
    assert.equal(input.requests.length, 3);
  } finally {
    await input.close();
  }
  for (const failure of ["notOpen", "storageFailed"]) {
    const refused = await fixture([new Response(JSON.stringify(failure), { status: 409 })]);
    try {
      await assert.rejects(
        client(refused).snapshot(),
        (error: unknown): boolean => error instanceof D4ControlFailure && error.failure === failure,
      );
      assert.equal(refused.requests.length, 1);
    } finally {
      await refused.close();
    }
  }
});

test("structured corruption keeps the table, row key and field without retry or coercion", async (): Promise<void> => {
  const corruptRow = { table: "central_work_attempts", key: subject, field: "state" };
  const input = await fixture([new Response(JSON.stringify({ corruptRow }), { status: 409 })]);
  try {
    await assert.rejects(client(input).snapshot(), (error: unknown): boolean => {
      assert.ok(error instanceof D4ControlFailure);
      assert.equal(error.failure, "corruptRow");
      assert.deepEqual(error.corruptRow, corruptRow);
      return true;
    });
    assert.equal(input.requests.length, 1);
  } finally {
    await input.close();
  }
  for (const reply of [
    { corruptRow: { table: "central_work_attempts", key: 7, field: "state" } },
    { corruptRow: { table: "central_work_attempts", key: subject } },
    "corruptRow",
  ]) {
    const malformed = await fixture([new Response(JSON.stringify(reply), { status: 409 })]);
    try {
      await assert.rejects(
        client(malformed).snapshot(),
        (error: unknown): boolean => !(error instanceof D4ControlFailure),
      );
      assert.equal(malformed.requests.length, 1);
    } finally {
      await malformed.close();
    }
  }
});

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

test("v3.8 settleClock uses the same exact settled reply on both shells", async (): Promise<void> => {
  for (const shell of ["tokio", "workerd"] as const) {
    const input = await fixture([{ kind: "clockSettled", value: { alarmEvents: 1n } }]);
    try {
      assert.equal(await client(input, { shell }).settleClock("2026-11-09T00:00:00Z"), 1n);
      assert.deepEqual(input.requests, [
        {
          address: { kind: "fleet" },
          command: { kind: "settleClock", value: { now: "2026-11-09T00:00:00Z" } },
        },
      ]);
    } finally {
      await input.close();
    }
  }
});

test("Tokio-only controls never leak onto workerd", async (): Promise<void> => {
  const input = await fixture([]);
  try {
    const controls = client(input, { shell: "workerd" });
    for (const call of [
      (): Promise<unknown> => controls.readPending(),
      (): Promise<unknown> => controls.readEvents(0n),
      (): Promise<unknown> => controls.duplicateAlarm(),
      (): Promise<unknown> => controls.rebuildAtQuiescence(true),
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

test("v3.4 names a bounded causal-wait failure distinctly and never retries it", async (): Promise<void> => {
  const input = await fixture([
    new Response('"waitBoundExceeded"', {
      status: 409,
      headers: { "content-type": "application/json" },
    }),
  ]);
  try {
    await assert.rejects(
      client(input).awaitBarrier(subject),
      (error: unknown): boolean =>
        error instanceof D4ControlFailure && error.failure === "waitBoundExceeded",
    );
    assert.equal(input.requests.length, 1);
  } finally {
    await input.close();
  }
});

test("the control HTTP deadline leaves room for the producer's named 30-second wait failure", async (): Promise<void> => {
  const input = await fixture([{ kind: "clockSettled", value: { alarmEvents: 0n } }]);
  const deadline = AbortSignal.timeout.bind(AbortSignal);
  const observed: number[] = [];
  const observation = mock.method(AbortSignal, "timeout", (milliseconds: number): AbortSignal => {
    observed.push(milliseconds);
    return deadline(milliseconds);
  });
  try {
    await client(input).settleClock("2026-11-08T01:00:00Z");
    assert.ok(
      observed.some((milliseconds): boolean => milliseconds > 30_000 && milliseconds <= 60_000),
    );
  } finally {
    observation.mock.restore();
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
