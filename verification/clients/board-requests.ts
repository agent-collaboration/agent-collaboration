import { randomBytes } from "node:crypto";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import type { Identity } from "../declaration.ts";
import { field } from "../exact-json.ts";
import { ObservationOpen } from "../receipt.ts";
import type { RouterMcp } from "./router-mcp.ts";

export const feedCursorSchema = z.object({
  epoch: z.bigint().nonnegative(),
  position: z.bigint().nonnegative(),
});
export type FeedCursor = z.infer<typeof feedCursorSchema>;
export const projectPlacementSchema = z.object({
  kind: z.literal("project"),
  position: z.object({ cursor: feedCursorSchema }),
});
export function feedAtLeast(actual: FeedCursor, expected: FeedCursor): boolean {
  return (
    actual.epoch > expected.epoch ||
    (actual.epoch === expected.epoch && actual.position >= expected.position)
  );
}

export function boardIdentity(): string {
  const bytes = randomBytes(16);
  bytes.writeUIntBE(Date.now(), 0, 6);
  bytes[6] = (bytes.readUInt8(6) & 15) | 112;
  bytes[8] = (bytes.readUInt8(8) & 63) | 128;
  const hex = bytes.toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
export function writeHeader(actor: Identity): Readonly<Record<string, unknown>> {
  return { requestId: boardIdentity(), protocolVersion: 1n, actingFor: null, actor };
}
const pendingSchema = z.object({
  result: z.object({ kind: z.enum(["pendingUnreachable", "projectCreationPending"]) }),
});
const acceptedSchema = z.object({
  result: z.object({
    kind: z.literal("decided"),
    decision: z.object({ kind: z.literal("accepted") }),
  }),
});
export async function acceptedWrite(input: {
  client: RouterMcp;
  tool: string;
  request: Readonly<Record<string, unknown>>;
}): Promise<unknown> {
  const deadline = Date.now() + 30_000;
  do {
    const reply = await input.client.call(input.tool, input.request);
    if (!pendingSchema.safeParse(reply).success) {
      acceptedSchema.parse(reply);
      return field(reply, "result");
    }
    await setTimeout(200);
  } while (Date.now() < deadline);
  throw new ObservationOpen("same-ID board request remains pending");
}
export const recordPageSchema = z.object({
  records: z.array(z.unknown()),
  nextCursor: z.string().nullable(),
});
export type RecordPage = z.infer<typeof recordPageSchema>;
export async function recordPages(input: {
  client: RouterMcp;
  tool: string;
  arguments: Readonly<Record<string, unknown>>;
  page: (reply: unknown) => RecordPage;
}): Promise<readonly unknown[]> {
  const records: unknown[] = [];
  let cursor: string | null = null;
  const seen = new Set<string>();
  for (let page = 0; page < 1000; page += 1) {
    const reply: RecordPage = input.page(
      await input.client.call(input.tool, {
        ...input.arguments,
        page: { limit: 100n, cursor },
      }),
    );
    records.push(...reply.records);
    if (reply.nextCursor === null) return records;
    if (seen.has(reply.nextCursor)) throw new Error("board page continuation repeated");
    seen.add(reply.nextCursor);
    cursor = reply.nextCursor;
  }
  throw new ObservationOpen("board record paging exceeded bound");
}
export async function projectRecords(input: {
  client: RouterMcp;
  project: string;
  tool: string;
}): Promise<readonly unknown[]> {
  return recordPages({
    ...input,
    arguments: { projectId: input.project },
    page: (reply): RecordPage => recordPageSchema.parse(reply),
  });
}
