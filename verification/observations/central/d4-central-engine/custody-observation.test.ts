import assert from "node:assert/strict";
import { test } from "node:test";
import { DatabaseSync } from "node:sqlite";
import { mkdir, mkdtemp, readFile, rm } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { decode } from "../../../exact-json.ts";
import { readD1Stores } from "../d1-sqlite-readback.ts";
import { custodyManifestFromSender, observeHeldCustody } from "./custody-observation.ts";

const digest = "a".repeat(64);
const itemId = (index: number): string =>
  `019f0000-0000-7000-8000-${String(index).padStart(12, "0")}`;
const createdAt = 1782411427840n;

function fixture(count = 10): Parameters<typeof observeHeldCustody>[0] {
  const items = Array.from({ length: count }, (_, index) => itemId(index + 1));
  return {
    journey: count === 10 ? "J6" : "J6b",
    expected: items.map((itemId) => ({ itemId, digest })),
    inboxReadback: {
      kind: "d1ClosedStoreReadback",
      stores: [
        {
          role: "recipient",
          snapshot: {
            readOnly: true,
            mail: items.map((item) => ({
              table: "_fleet_sync_inbox_items",
              item,
              rows: [
                {
                  item_id: item,
                  digest,
                  created_at: createdAt,
                  accepted_at: createdAt + 5n,
                  progress: "held",
                  payloadPresent: true,
                  payload_bytes: 123n,
                },
              ],
            })),
          },
        },
      ],
    },
    recipientViews: items.map((pushId) => ({ record: { pushId, deliveryState: "held" } })),
    senderViews: items.map((item) => ({ mail: { item, status: { kind: "held" } } })),
  };
}

test("J6 and J6b require every declared identity at all three custody readbacks", (): void => {
  for (const count of [10, 1001]) {
    const input = fixture(count);
    const actual = observeHeldCustody(input);
    assert.equal(actual.inboxRows, count);
    assert.deepEqual(
      actual.items,
      input.expected.map((item) => item.itemId),
    );
    assert.equal(actual.runtimeQualified, false);
  }
});

test("equal aggregate counts cannot hide a missing item behind a duplicate status or push", (): void => {
  const input = fixture();
  const duplicate = (values: readonly unknown[]): unknown[] => [values[0], ...values.slice(0, -1)];
  assert.throws(
    () => observeHeldCustody({ ...input, senderViews: duplicate(input.senderViews) }),
    /exactly one/,
  );
  const first = input.expected[0];
  assert.ok(first);
  assert.throws(
    () => observeHeldCustody({ ...input, recipientViews: duplicate(input.recipientViews) }),
    /exactly one/,
  );
  assert.throws(
    () =>
      observeHeldCustody({
        ...input,
        expected: [...input.expected.slice(0, -1), first],
      }),
    /distinct declared/,
  );
  assert.throws(() => observeHeldCustody({ ...input, journey: "J6b" }), /1001 distinct/);
});

test("a held public push and sender status cannot replace a real held inbox row", (): void => {
  const input = fixture();
  for (const rows of [
    [],
    [{ item_id: itemId(1) }],
    [
      {
        item_id: itemId(1),
        digest,
        created_at: createdAt,
        accepted_at: createdAt + 5n,
        progress: "final",
        payloadPresent: false,
        payload_bytes: 0n,
      },
    ],
  ]) {
    const inboxReadback = {
      kind: "d1ClosedStoreReadback",
      stores: [
        {
          role: "recipient",
          snapshot: {
            readOnly: true,
            mail: input.expected.map((item, index) => ({
              table: "_fleet_sync_inbox_items",
              item: item.itemId,
              rows:
                index === 0
                  ? rows
                  : [
                      {
                        item_id: item.itemId,
                        digest,
                        created_at: createdAt,
                        accepted_at: createdAt + 5n,
                        progress: "held",
                        payloadPresent: true,
                        payload_bytes: 123n,
                      },
                    ],
            })),
          },
        },
      ],
    };
    assert.throws(() => observeHeldCustody({ ...input, inboxReadback }));
  }
});

test("malformed owner/readback, missing status, wrong item and changed digest all refuse", (): void => {
  const input = fixture();
  assert.throws(() =>
    observeHeldCustody({
      ...input,
      inboxReadback: { kind: "snapshot", stores: [] },
    }),
  );
  assert.throws(
    () => observeHeldCustody({ ...input, senderViews: input.senderViews.slice(1) }),
    /sender Held/,
  );
  assert.throws(() =>
    observeHeldCustody({
      ...input,
      recipientViews: [{ record: { pushId: itemId(1), deliveryState: "delivered" } }],
    }),
  );
  assert.throws(
    () =>
      observeHeldCustody({
        ...input,
        expected: input.expected.map((item) => ({ ...item, digest: "b".repeat(64) })),
      }),
    /digest differs/,
  );
});

test("row identity, creation time, payload and owner markers remain mandatory", (): void => {
  const input = fixture();
  for (const changes of [
    { item_id: itemId(2) },
    { digest: "b".repeat(64) },
    { created_at: createdAt + 1n },
    { accepted_at: createdAt - 1n },
    { created_at: Number(createdAt) },
    { payloadPresent: false },
    { payload_bytes: 0n },
  ]) {
    const inboxReadback = {
      kind: "d1ClosedStoreReadback",
      stores: [
        {
          role: "recipient",
          snapshot: {
            readOnly: true,
            mail: input.expected.map((item) => ({
              table: "_fleet_sync_inbox_items",
              item: item.itemId,
              rows: [
                {
                  item_id: item.itemId,
                  digest,
                  created_at: createdAt,
                  accepted_at: createdAt + 5n,
                  progress: "held",
                  payloadPresent: true,
                  payload_bytes: 123n,
                  ...changes,
                },
              ],
            })),
          },
        },
      ],
    };
    assert.throws(() => observeHeldCustody({ ...input, inboxReadback }));
  }
  for (const stores of [
    [],
    [{ role: "fleet", snapshot: { readOnly: true, mail: [] } }],
    [{ role: "recipient", snapshot: { readOnly: false, mail: [] } }],
  ])
    assert.throws(() =>
      observeHeldCustody({ ...input, inboxReadback: { kind: "d1ClosedStoreReadback", stores } }),
    );
});

test("distinct custody snapshots may be reordered without imposing global delivery order", (): void => {
  const input = fixture();
  const actual = observeHeldCustody({
    ...input,
    recipientViews: [...input.recipientViews].reverse(),
    senderViews: [...input.senderViews].reverse(),
  });
  assert.equal(actual.inboxRows, 10);
});

test("custody reader consumes the actual closed-store producer's lossless file format", async (): Promise<void> => {
  const repository = fileURLToPath(new URL("../../../..", import.meta.url)).replace(/\/$/, "");
  await mkdir(`${repository}/tmp/d1-verify`, { recursive: true, mode: 0o700 });
  const root = await mkdtemp(`${repository}/tmp/d1-verify/d4-custody-reader-`);
  const input = fixture();
  const database = `${root}/inbox.sqlite`;
  const owner = new DatabaseSync(database);
  try {
    // Minimal apparatus fixture, not a product migration or owner-store proof.
    owner.exec(
      "CREATE TABLE _fleet_sync_inbox_items(item_id TEXT PRIMARY KEY,digest TEXT,created_at INTEGER,accepted_at INTEGER,progress TEXT,payload TEXT,payload_bytes INTEGER)",
    );
    owner.exec(
      "CREATE TABLE _fleet_sync_outbox_items(item_id TEXT PRIMARY KEY,digest TEXT,payload TEXT)",
    );
    const insert = owner.prepare("INSERT INTO _fleet_sync_inbox_items VALUES (?,?,?,?,?,?,?)");
    for (const item of input.expected)
      insert.run(
        item.itemId,
        item.digest,
        createdAt,
        createdAt + 5n,
        "held",
        "private fixture payload",
        23n,
      );
    const sent = owner.prepare("INSERT INTO _fleet_sync_outbox_items VALUES (?,?,?)");
    for (const item of input.expected) sent.run(item.itemId, item.digest, null);
  } finally {
    owner.close();
  }
  try {
    const output = `${root}/readback.json`;
    await readD1Stores({
      repository,
      databases: [
        { role: "sender", path: database },
        { role: "recipient", path: database },
      ],
      itemIds: input.expected.map((item): string => item.itemId),
      output,
    });
    const contents = await readFile(output, "utf8");
    assert.ok(!contents.includes("private fixture payload"));
    const raw = decode(contents);
    const expected = custodyManifestFromSender({
      itemIds: input.expected.map((item): string => item.itemId),
      readback: raw,
    });
    assert.deepEqual(expected, input.expected);
    const result = observeHeldCustody({ ...input, expected, inboxReadback: raw });
    assert.equal(result.inboxRows, 10);
    assert.equal(result.runtimeQualified, false);
  } finally {
    await rm(root, { recursive: true });
  }
});

test("custody digests come from exactly one matching sender row, not a recipient snapshot", (): void => {
  const input = fixture();
  const itemIds = input.expected.map((item): string => item.itemId);
  assert.throws(
    () => custodyManifestFromSender({ itemIds, readback: input.inboxReadback }),
    /sender store/,
  );
  const readback = {
    kind: "d1ClosedStoreReadback",
    stores: [
      {
        role: "sender",
        snapshot: {
          readOnly: true,
          mail: input.expected.map((item) => ({
            table: "_fleet_sync_outbox_items",
            item: item.itemId,
            rows: [{ item_id: item.itemId, digest: item.digest }],
          })),
        },
      },
    ],
  };
  assert.deepEqual(custodyManifestFromSender({ itemIds, readback }), input.expected);
  assert.throws(
    () => custodyManifestFromSender({ itemIds: [...itemIds, itemIds[0] ?? ""], readback }),
    /duplicate/,
  );
  assert.throws(
    () => custodyManifestFromSender({ itemIds: [...itemIds.slice(1), itemId(100)], readback }),
    /sender outbox entry/,
  );
});
