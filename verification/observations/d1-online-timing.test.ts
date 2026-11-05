import assert from "node:assert/strict";
import { test } from "node:test";
import { decode, encode } from "../exact-json.ts";
import { timingFromD1Readback } from "./d1-online-timing.ts";

const items = Array.from(
  { length: 20 },
  (_, index): string => `018bcfe5-6800-7000-8000-${(index + 1).toString(16).padStart(12, "0")}`,
);
function fixture(table = "_fleet_sync_inbox_items", missing = false): unknown {
  return decode(
    encode({
      stores: [
        {
          role: "recipient",
          snapshot: {
            readOnly: true,
            mail: items.map((item, index) => ({
              table,
              item,
              rows:
                missing && index === 3
                  ? []
                  : [
                      {
                        item_id: item,
                        digest: "a".repeat(64),
                        created_at: 1_700_000_000_000n,
                        accepted_at: 1_700_000_000_000n + BigInt(index),
                        payloadPresent: true,
                        payload_bytes: 768n,
                        progress: "final",
                        outcome_kind: "refused",
                        refusal_kind: "recipientUnknown",
                      },
                    ],
            })),
          },
        },
      ],
    }),
  );
}

test("online timing maps actual inbox timestamps to twenty samples and their median", (): void => {
  const actual = timingFromD1Readback(fixture(), items);
  assert.deepEqual(actual.report, {
    sampleMilliseconds: Array.from({ length: 20 }, (_, index): number => index),
    medianMilliseconds: 9.5,
    observation: "acceptedAtMinusCreatedAt",
    latencyThreshold: null,
  });
  assert.equal(actual.inboxOutcomes.length, 20);
  assert.equal(actual.inboxOutcomes[0]?.refusalKind, "recipientUnknown");
});

test("outbox timing, a missing recipient row and duplicate sample IDs cannot supply inbox proof", (): void => {
  assert.throws(() => timingFromD1Readback(fixture("_fleet_sync_outbox_items"), items));
  assert.throws(() => timingFromD1Readback(fixture("_fleet_sync_inbox_items", true), items));
  assert.throws(() => timingFromD1Readback(fixture(), [...items.slice(1), items[1] ?? ""]));
});
