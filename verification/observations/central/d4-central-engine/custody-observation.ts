/** Compare a declared workload with retained Router custody and public status readbacks. */
import { z } from "zod";
import { itemCreationMilliseconds } from "../d1-mail-evidence.ts";

const itemIdentity = z.uuidv7();
const expectedItemSchema = z
  .object({ itemId: itemIdentity, digest: z.string().regex(/^[a-f0-9]{64}$/) })
  .strict();
const inboxRowSchema = z.object({
  item_id: itemIdentity,
  digest: z.string().regex(/^[a-f0-9]{64}$/),
  created_at: z.bigint().nonnegative(),
  accepted_at: z.bigint().nonnegative(),
  progress: z.literal("held"),
  payloadPresent: z.literal(true),
  payload_bytes: z.bigint().positive(),
});
const readbackSchema = z.object({
  kind: z.literal("d1ClosedStoreReadback"),
  stores: z.array(
    z.object({
      role: z.enum(["fleet", "sender", "recipient"]),
      snapshot: z.object({
        readOnly: z.literal(true),
        mail: z.array(
          z.object({ table: z.string(), item: itemIdentity, rows: z.array(z.unknown()) }),
        ),
      }),
    }),
  ),
});
const recipientViewSchema = z.object({
  record: z.object({ pushId: itemIdentity, deliveryState: z.literal("held") }),
});
const senderViewSchema = z.object({
  mail: z.object({ item: itemIdentity, status: z.object({ kind: z.literal("held") }) }),
});
export type ExpectedCustodyItem = z.infer<typeof expectedItemSchema>;

export interface HeldCustodyObservation {
  readonly journey: "J6" | "J6b";
  readonly items: readonly string[];
  readonly inboxRows: number;
  readonly recipientHeldViews: number;
  readonly senderHeldViews: number;
  readonly runtimeQualified: false;
}

function oneMatching<TValue>(
  values: readonly TValue[],
  matches: (value: TValue) => boolean,
  label: string,
): TValue {
  const selected = values.filter(matches);
  if (selected.length !== 1 || selected[0] === undefined)
    throw new Error(`exactly one ${label} required`);
  return selected[0];
}

/** Compare recipient content with the sender's retained digest, never with the recipient itself. */
export function custodyManifestFromSender(input: {
  readonly itemIds: readonly string[];
  readonly readback: unknown;
}): readonly ExpectedCustodyItem[] {
  const items = z.array(itemIdentity).min(1).parse(input.itemIds);
  if (new Set(items).size !== items.length) throw new Error("duplicate submitted custody item");
  const sender = oneMatching(
    readbackSchema.parse(input.readback).stores,
    (store): boolean => store.role === "sender",
    "sender store",
  );
  const outbox = sender.snapshot.mail.filter(
    (entry): boolean => entry.table === "_fleet_sync_outbox_items",
  );
  return items.map((item): ExpectedCustodyItem => {
    const entry = oneMatching(
      outbox,
      (entry): boolean => entry.item === item,
      `sender outbox entry for ${item}`,
    );
    if (entry.rows.length !== 1)
      throw new Error(`exactly one sender outbox row required for ${item}`);
    const row = z
      .object({ item_id: itemIdentity, digest: expectedItemSchema.shape.digest })
      .parse(entry.rows[0]);
    if (row.item_id !== item) throw new Error("sender outbox row names another item");
    return { itemId: row.item_id, digest: row.digest };
  });
}

/** The caller retains original readbacks and qualifies their target and capture provenance. */
export function observeHeldCustody(input: {
  readonly journey: "J6" | "J6b";
  readonly expected: readonly ExpectedCustodyItem[];
  readonly inboxReadback: unknown;
  readonly recipientViews: readonly unknown[];
  readonly senderViews: readonly unknown[];
}): HeldCustodyObservation {
  const journey = z.enum(["J6", "J6b"]).parse(input.journey);
  const expected = z.array(expectedItemSchema).parse(input.expected);
  const requiredCount = journey === "J6" ? 10 : 1001;
  if (
    expected.length !== requiredCount ||
    new Set(expected.map((item) => item.itemId)).size !== requiredCount
  )
    throw new Error(`${journey} requires ${requiredCount} distinct declared items`);
  const readback = readbackSchema.parse(input.inboxReadback);
  const recipient = oneMatching(
    readback.stores,
    (store): boolean => store.role === "recipient",
    "recipient store",
  );
  const inbox = recipient.snapshot.mail.filter(
    (entry): boolean => entry.table === "_fleet_sync_inbox_items",
  );
  const recipientViews = z.array(recipientViewSchema).parse(input.recipientViews);
  const senderViews = z.array(senderViewSchema).parse(input.senderViews);
  for (const item of expected) {
    const entry = oneMatching(
      inbox,
      (entry): boolean => entry.item === item.itemId,
      `inbox entry for ${item.itemId}`,
    );
    if (entry.rows.length !== 1)
      throw new Error(`exactly one inbox row required for ${item.itemId}`);
    const row = inboxRowSchema.parse(entry.rows[0]);
    if (row.item_id !== item.itemId || row.digest !== item.digest)
      throw new Error(`inbox identity or digest differs for ${item.itemId}`);
    if (
      row.created_at !== BigInt(itemCreationMilliseconds(item.itemId)) ||
      row.accepted_at < row.created_at
    )
      throw new Error(`inbox creation or acceptance time differs for ${item.itemId}`);
    oneMatching(
      recipientViews,
      (view): boolean => view.record.pushId === item.itemId,
      `recipient Held view for ${item.itemId}`,
    );
    oneMatching(
      senderViews,
      (view): boolean => view.mail.item === item.itemId,
      `sender Held status for ${item.itemId}`,
    );
  }
  return {
    journey,
    items: expected.map((item): string => item.itemId),
    inboxRows: expected.length,
    recipientHeldViews: expected.length,
    senderHeldViews: expected.length,
    runtimeQualified: false,
  };
}
