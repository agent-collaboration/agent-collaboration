import { test } from "node:test";
import assert from "node:assert/strict";
import { z } from "zod";
import { fixture } from "./control-fixture.ts";
import { D4ControlClient } from "./control-client.ts";
import { ClaimOverlapFailure, driveTokioClaimOverlap } from "./claim-overlap.ts";

function identity(index: number): string {
  return `01900000-0000-7000-8000-${index.toString(16).padStart(12, "0")}`;
}
function event(index: number, claimed: boolean) {
  return {
    object: { kind: "fleet" },
    construction: identity(1),
    sequence: BigInt(1 - index),
    eventId: identity(200 + index),
    event: { kind: "alarm", redelivery: 1n },
    enteredAt: 1794096000000n,
    exitedAt: 1794096000010n,
    units: [],
    sends: [],
    notices: [],
    calls: [],
    alarm: { before: null, after: null, writes: [] },
    batch: claimed
      ? { claimed: true, startedAt: 1794096000000n, endedAt: 1794096000001n, sources: [] }
      : { claimed: false },
    reply: { kind: "alarmDone" },
  };
}
type Fault =
  | "none"
  | "same-event"
  | "two-claims"
  | "zero-claims"
  | "missing-event-binding"
  | "uncertain-release";
async function overlapFixture(fault: Fault = "none") {
  const latches = [0, 1].map((index) => ({
    id: identity(100 + index),
    eventId: identity(200 + index),
    entered: false,
    reached: Promise.withResolvers<void>(),
    released: Promise.withResolvers<void>(),
  }));
  let armed = 0;
  let entries = 0;
  let reads = 0;
  let finished = 0;
  let releases = 0;
  let releasedBeforeBoth = false;
  const http = await fixture(async (raw): Promise<unknown> => {
    const command = z
      .object({ command: z.object({ kind: z.string(), value: z.unknown().optional() }) })
      .parse(raw).command;
    if (command.kind === "readPending")
      return { kind: "pending", value: { tasks: 0n, deadlines: 0n, waiters: 0n } };
    if (command.kind === "readEvents") {
      reads += 1;
      const records =
        reads === 1
          ? []
          : [event(1, fault === "two-claims"), event(0, fault !== "zero-claims")].map(
              (record, index) => ({ cursor: BigInt(index + 1), record }),
            );
      return {
        kind: "events",
        value: { records, firstRetained: 1n, through: BigInt(records.length) },
      };
    }
    if (command.kind === "barrier") {
      const latch = latches[armed++];
      assert.ok(latch);
      assert.deepEqual(command.value, { point: "beforeClaim", unitKind: null });
      return { kind: "barrierArmed", value: { barrierId: latch.id } };
    }
    if (command.kind === "duplicateAlarm") {
      const latch = latches[entries++];
      assert.ok(latch);
      latch.entered = true;
      latch.reached.resolve();
      await latch.released.promise;
      finished += 1;
      return { kind: "done" };
    }
    const id = z.object({ barrierId: z.uuidv7() }).parse(command.value).barrierId;
    const latch = latches.find((value): boolean => value.id === id);
    assert.ok(latch);
    if (command.kind === "awaitBarrier") {
      await latch.reached.promise;
      return {
        kind: "barrier",
        value:
          fault === "missing-event-binding"
            ? { reached: true }
            : { reached: true, eventId: fault === "same-event" ? identity(200) : latch.eventId },
      };
    }
    assert.equal(command.kind, "releaseBarrier");
    releases += 1;
    releasedBeforeBoth ||= !latches.every((value): boolean => value.entered);
    latch.released.resolve();
    return fault === "uncertain-release" ? { kind: "unexpectedAnswer" } : { kind: "done" };
  });
  return {
    http,
    client: new D4ControlClient({
      owner: http.owner,
      address: { kind: "fleet" },
      shell: "tokio",
      passivity: "observed",
      clockPassivity: "observed",
    }),
    state: () => ({ armed, entries, finished, releases, releasedBeforeBoth }),
  };
}

test("J18 waits for two distinct held event identities before release and joins out-of-order completions", async (): Promise<void> => {
  const input = await overlapFixture();
  try {
    const result = await driveTokioClaimOverlap(input.client);
    assert.deepEqual(result.heldEvents, [identity(200), identity(201)]);
    assert.deepEqual(
      result.records.map((record): string => record.eventId),
      [identity(201), identity(200)],
    );
    assert.equal(result.runtimeQualified, false);
    assert.deepEqual(input.state(), {
      armed: 2,
      entries: 2,
      finished: 2,
      releases: 2,
      releasedBeforeBoth: false,
    });
  } finally {
    await input.http.close();
  }
});

test("zero or two actual claims fail the semantic no-duplicate-batch assertion", async (): Promise<void> => {
  for (const fault of ["two-claims", "zero-claims"] as const) {
    const input = await overlapFixture(fault);
    try {
      await assert.rejects(
        driveTokioClaimOverlap(input.client),
        (error: unknown): boolean =>
          error instanceof ClaimOverlapFailure && error.check === "no-duplicate-batch",
      );
      assert.equal(input.state().finished, 2);
    } finally {
      await input.http.close();
    }
  }
});

test("one event reported by two latches cannot establish overlap and both owned actions still settle", async (): Promise<void> => {
  const input = await overlapFixture("same-event");
  try {
    await assert.rejects(
      driveTokioClaimOverlap(input.client),
      (error: unknown): boolean =>
        error instanceof ClaimOverlapFailure && error.check === "overlapping-claim",
    );
    assert.equal(input.state().releases, 2);
    assert.equal(input.state().finished, 2);
  } finally {
    await input.http.close();
  }
});

test("legacy reached-only answers fail setup while every captured event is released", async (): Promise<void> => {
  const input = await overlapFixture("missing-event-binding");
  try {
    await assert.rejects(
      driveTokioClaimOverlap(input.client),
      (error: unknown): boolean => !(error instanceof ClaimOverlapFailure),
    );
    assert.equal(input.state().releases, 2);
    assert.equal(input.state().finished, 2);
  } finally {
    await input.http.close();
  }
});

test("an uncertain release prevents credit and is never replayed", async (): Promise<void> => {
  const input = await overlapFixture("uncertain-release");
  try {
    await assert.rejects(driveTokioClaimOverlap(input.client), AggregateError);
    assert.equal(input.state().releases, 2);
    assert.equal(input.state().finished, 2);
  } finally {
    await input.http.close();
  }
});
