import assert from "node:assert/strict";
import { test } from "node:test";
import { boundedObservationResultSchema } from "../../mcp-control/observations/observation-models.ts";
import type { ObservationRead } from "../../mcp-control/observations/observation-ledger.ts";
import { captureProviderPages, providerPageSummary, type PageCursor } from "./observation-pages.ts";

const target = {
  endpoint: { serviceId: "service-a", endpointId: "claude-local" },
  sessionId: "owned-session",
};

function page(cursor: PageCursor, sequences: readonly bigint[], limited: boolean): ObservationRead {
  return {
    request: cursor,
    result: boundedObservationResultSchema.parse({
      target,
      generation: {},
      attached: true,
      epoch: 3n,
      continuationGap: true,
      endReason: limited ? "resultLimitReached" : "deadlineReached",
      events: sequences.map((sequence): unknown => ({
        sequence,
        event: { kind: "stateChanged", state: "idle" },
      })),
    }),
    notifications: [],
  };
}

test("a multi-page baseline drains before input and continuation begins after its actual cursor", async (): Promise<void> => {
  const cursors: PageCursor[] = [];
  const sequences = [[1n, 2n], [3n], [4n, 5n]];
  const readPage = async (cursor: PageCursor): Promise<ObservationRead> => {
    cursors.push(cursor);
    const next = sequences.shift();
    if (next === undefined) throw new Error("unexpected read");
    return page(cursor, next, next[0] === 1n);
  };
  const recorded: number[] = [];
  const baseline = await captureProviderPages({
    target,
    previous: null,
    readPage,
    recordPage: (_read, index): void => {
      recorded.push(index);
    },
  });
  const continued = await captureProviderPages({
    target,
    previous: baseline,
    readPage,
    recordPage: (_read, index): void => {
      recorded.push(index);
    },
  });
  assert.deepEqual(cursors, [
    { epoch: null, afterSequence: null },
    { epoch: 3n, afterSequence: 2n },
    { epoch: 3n, afterSequence: 3n },
  ]);
  assert.deepEqual(recorded, [0, 1, 2]);
  assert.equal(continued.baselineReads, 2);
  assert.deepEqual(
    continued.ledger.events.map((event): bigint => event.sequence),
    [4n, 5n],
  );
  assert.equal(providerPageSummary(continued)["productVerdict"], "notEvaluated");
});

test("an undrained page bound stops before more input and keeps the limited page", async (): Promise<void> => {
  let recorded = 0;
  await assert.rejects(
    captureProviderPages({
      target,
      previous: null,
      maxPages: 1,
      readPage: async (cursor): Promise<ObservationRead> => page(cursor, [1n], true),
      recordPage: (): void => {
        recorded += 1;
      },
    }),
    /page bound exhausted/,
  );
  assert.equal(recorded, 1);
});

test("a provider that replays old events instead of following the cursor cannot pass", async (): Promise<void> => {
  let reads = 0;
  await assert.rejects(
    captureProviderPages({
      target,
      previous: null,
      readPage: async (cursor): Promise<ObservationRead> => {
        reads += 1;
        return page(cursor, [1n], reads === 1);
      },
      recordPage: (): void => {},
    }),
    /rejected/,
  );
  assert.equal(reads, 2);
});

test("resync and oversized events remain hard coverage gaps", async (): Promise<void> => {
  for (const marker of [
    { kind: "resyncRequired" },
    { kind: "eventTooLarge", sequence: 1n, itemId: null },
  ]) {
    await assert.rejects(
      captureProviderPages({
        target,
        previous: null,
        readPage: async (cursor): Promise<ObservationRead> => ({
          ...page(cursor, [], false),
          result: boundedObservationResultSchema.parse({
            ...page(cursor, [], false).result,
            events: [marker],
            endReason: marker.kind === "resyncRequired" ? "resyncRequired" : "deadlineReached",
          }),
        }),
        recordPage: (): void => {},
      }),
      /resyncRequired|oversized-event gap/,
    );
  }
});

test("a binding change cannot be hidden behind an unchanged hub epoch", async (): Promise<void> => {
  const baseline = await captureProviderPages({
    target,
    previous: null,
    readPage: async (cursor): Promise<ObservationRead> => page(cursor, [1n], false),
    recordPage: (): void => {},
  });
  await assert.rejects(
    captureProviderPages({
      target,
      previous: baseline,
      readPage: async (cursor): Promise<ObservationRead> => {
        const result = page(cursor, [2n], false);
        return { ...result, result: { ...result.result, generation: { changed: true } } };
      },
      recordPage: (): void => {},
    }),
    /binding generation changed/,
  );
});
