/** Twenty real online queue items, measured from the recipient's settled inbox rows. */
import { readFile } from "node:fs/promises";
import { z } from "zod";
import type { D1Machine } from "./d1-harness.ts";
import type { Receipt } from "../receipt.ts";
import { boardIdentity } from "../clients/board-requests.ts";
import { decode } from "../exact-json.ts";
import {
  inboxTimingReport,
  mailRecordEvidenceSchema,
  type InboxTimingReport,
  type MailRecordEvidence,
} from "./d1-mail-evidence.ts";
import { readD1Stores } from "./d1-sqlite-readback.ts";
import { requireNoD1Turns, senderStage, unloadD1Recipient } from "./d1-mail-stages.ts";

export async function driveD1OnlineTiming(input: {
  sender: D1Machine;
  recipient: D1Machine;
  receipt: Receipt;
}): Promise<readonly string[]> {
  const { sender, recipient, receipt } = input;
  await unloadD1Recipient(recipient, receipt);
  const items: string[] = [];
  for (let sample = 0; sample < 20; sample += 1) {
    const item = boardIdentity();
    const request = {
      itemId: item,
      target: recipient.recipient,
      delivery: "queue",
      message: { kind: "agent", sender: sender.recipient, text: `D1 timing fixture ${item}.` },
    };
    await receipt.record("d1TimingRequest", { sample, request });
    const submitted = await sender.mcp.call("message_send", request);
    z.object({ queued: z.object({ item: z.literal(item) }) }).parse(submitted);
    await receipt.record("d1TimingAdmission", { sample, submitted });
    await senderStage({
      sender,
      item,
      kinds: ["held", "delivered", "refused", "unknown"],
      receipt,
      stage: `timing-${sample}`,
    });
    await requireNoD1Turns([sender, recipient], receipt);
    items.push(item);
  }
  await receipt.save("timing-items.json", { items, hostsOnline: true, recipientLoaded: false });
  return items;
}

const inboxRow = z.object({
  item_id: z.uuidv7(),
  digest: z.string(),
  created_at: z.bigint(),
  accepted_at: z.bigint(),
  payloadPresent: z.boolean(),
  payload_bytes: z.bigint().nonnegative(),
  progress: z.string(),
  outcome_kind: z.string().nullable(),
  refusal_kind: z.string().nullable(),
});

export function timingFromD1Readback(
  raw: unknown,
  items: readonly string[],
): {
  records: MailRecordEvidence[];
  report: InboxTimingReport;
  stagingExecuted: false;
  inboxOutcomes: {
    itemId: string;
    progress: string;
    outcomeKind: string | null;
    refusalKind: string | null;
  }[];
} {
  const readback = z
    .object({
      stores: z
        .array(
          z.object({
            role: z.literal("recipient"),
            snapshot: z.object({
              readOnly: z.literal(true),
              mail: z.array(
                z.object({ table: z.string(), item: z.uuidv7(), rows: z.array(z.unknown()) }),
              ),
            }),
          }),
        )
        .length(1),
    })
    .parse(raw);
  if (items.length !== 20 || new Set(items).size !== 20)
    throw new Error("twenty distinct requested timing items required");
  const inbox =
    readback.stores[0]?.snapshot.mail.filter(
      (entry): boolean => entry.table === "_fleet_sync_inbox_items",
    ) ?? [];
  const rows = items.map((item) => {
    const matching = inbox.filter((entry): boolean => entry.item === item);
    if (matching.length !== 1 || matching[0]?.rows.length !== 1)
      throw new Error("exactly one real recipient inbox row required for each timing item");
    const row = inboxRow.parse(matching[0].rows[0]);
    if (row.item_id !== item) throw new Error("inbox timing item identity changed");
    return row;
  });
  const records = rows.map((row) =>
    mailRecordEvidenceSchema.parse({
      itemId: row.item_id,
      digest: row.digest,
      createdAt: new Date(Number(row.created_at)).toISOString(),
      acceptedAt: new Date(Number(row.accepted_at)).toISOString(),
      payloadPresent: row.payloadPresent,
      payloadBytes: Number(row.payload_bytes),
    }),
  );
  return {
    records,
    report: inboxTimingReport(records),
    stagingExecuted: false,
    inboxOutcomes: rows.map((row) => ({
      itemId: row.item_id,
      progress: row.progress,
      outcomeKind: row.outcome_kind,
      refusalKind: row.refusal_kind,
    })),
  };
}

export async function reportD1OnlineTiming(input: {
  repository: string;
  recipientDirectory?: string | undefined;
  runtimeDirectory?: string | undefined;
  items: readonly string[];
  receipt: Receipt;
}): Promise<string | null> {
  try {
    const output = `${input.receipt.directory}/timing-inbox-readback.json`;
    await readD1Stores({
      repository: input.repository,
      databases: [
        {
          role: "recipient",
          path: `${input.recipientDirectory ?? `${input.repository}/tmp/b`}/interaction.sqlite`,
        },
      ],
      runtimeDirectory: input.runtimeDirectory,
      itemIds: [...input.items],
      output,
    });
    const readback = decode(await readFile(output, "utf8"));
    await input.receipt.save("timing-report.json", timingFromD1Readback(readback, input.items));
    return null;
  } catch (error: unknown) {
    return error instanceof Error ? error.message : "unknown timing readback failure";
  }
}
