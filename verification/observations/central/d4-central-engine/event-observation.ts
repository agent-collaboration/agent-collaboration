/** Lossless S1/S4 observations from the Implementer's D4 seam contract v3.
 * Draft until the corresponding product slice is declared and its real bytes qualify this reader.
 */
import { z } from "zod";
import { decode } from "../../../exact-json.ts";

export const d4ObjectAddressSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("fleet") }).strict(),
  z.object({ kind: z.literal("copy"), value: z.uuidv7() }).strict(),
]);
export type D4ObjectAddress = z.infer<typeof d4ObjectAddressSchema>;
export const unsigned64Schema = z.bigint().nonnegative().max(18446744073709551615n);
export const epochMillisecondsSchema = z.bigint().nonnegative().max(9223372036854775807n);
const label = z.string().min(1);
const exactBytes = z
  .string()
  .min(1)
  .refine(
    (value): boolean => Buffer.from(value, "base64").toString("base64") === value,
    "canonical original base64 bytes required",
  );
const eventSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("socketOpened"), connection: z.uuidv7() }).strict(),
  z.object({ kind: z.literal("frame"), connection: z.uuidv7(), frame: exactBytes }).strict(),
  z
    .object({
      kind: z.literal("socketClosed"),
      connection: z.uuidv7(),
      cause: z.enum(["callback", "inventoryReconcile"]),
    })
    .strict(),
  z.object({ kind: z.literal("call"), callKind: label }).strict(),
  z
    .object({
      kind: z.literal("alarm"),
      redelivery: z.bigint().nonnegative().max(4294967295n).nullable(),
    })
    .strict(),
]);
const unitSchema = z
  .object({
    unitKind: label,
    subject: z.uuidv7(),
    outcome: z.discriminatedUnion("kind", [
      z.object({ kind: z.literal("done") }).strict(),
      z.object({ kind: z.literal("deferred"), retryAt: epochMillisecondsSchema }).strict(),
      z.object({ kind: z.literal("failed"), errorClass: label, message: label }).strict(),
    ]),
  })
  .strict();
const sendBatchSchema = z
  .object({
    batchSendId: z.uuidv7(),
    index: z.bigint().nonnegative(),
    items: z.bigint().min(1n).max(32n),
  })
  .strict()
  .refine((batch): boolean => batch.index < batch.items, "batch index outside item count");
const sendSchema = z
  .object({
    attempt: z.uuidv7(),
    attemptKind: label,
    subject: z.uuidv7(),
    sendId: z.uuidv7(),
    connection: z.uuidv7(),
    session: z.uuidv7(),
    requestKind: label,
    sentVersion: unsigned64Schema.nullable(),
    batch: sendBatchSchema.nullable(),
    frame: exactBytes,
  })
  .strict()
  .refine(
    (send): boolean => (send.attemptKind === "status") === (send.sentVersion !== null),
    "only status sends carry sentVersion",
  );
const alarmWriteSchema = z.discriminatedUnion("op", [
  z.object({ op: z.literal("lower"), at: epochMillisecondsSchema, unitKind: label }).strict(),
  z.object({ op: z.literal("exact"), at: epochMillisecondsSchema.nullable() }).strict(),
]);
const sourceSchema = z
  .object({
    source: z.enum([
      "attempt_retries",
      "notice_expiry",
      "mail_retention",
      "forward_expiry",
      "feed_retention",
      "staged_part_expiry",
      "long_operations",
    ]),
    done: unsigned64Schema,
    failed: unsigned64Schema,
    remaining: unsigned64Schema,
  })
  .strict();
const batchSchema = z.discriminatedUnion("claimed", [
  z
    .object({
      claimed: z.literal(true),
      startedAt: epochMillisecondsSchema,
      endedAt: epochMillisecondsSchema,
      sources: z.array(sourceSchema),
    })
    .strict(),
  // The unclaimed case promises only the false decision; timing fields are not used as evidence.
  z.object({ claimed: z.literal(false) }).passthrough(),
]);
export const d4EventRecordSchema = z
  .object({
    object: d4ObjectAddressSchema,
    construction: z.uuidv7(),
    sequence: unsigned64Schema,
    eventId: z.uuidv7(),
    event: eventSchema,
    enteredAt: epochMillisecondsSchema,
    exitedAt: epochMillisecondsSchema,
    units: z.array(unitSchema),
    sends: z.array(sendSchema),
    notices: z.array(
      z
        .object({
          machine: z.uuidv7(),
          from: unsigned64Schema,
          through: unsigned64Schema,
          cause: z.enum(["live", "welcome", "resend"]),
        })
        .strict()
        .refine((notice): boolean => notice.from <= notice.through, "notice range is reversed"),
    ),
    calls: z.array(
      z
        .object({
          to: d4ObjectAddressSchema,
          callKind: label,
          unitKind: label,
          subject: z.uuidv7(),
          outcome: z.enum(["answered", "deadlineExceeded", "failed"]),
        })
        .strict(),
    ),
    alarm: z
      .object({
        before: epochMillisecondsSchema.nullable(),
        after: epochMillisecondsSchema.nullable(),
        writes: z.array(alarmWriteSchema),
      })
      .strict(),
    batch: batchSchema.nullable(),
    reply: z
      .object({ kind: z.enum(["none", "callAnswer", "callFailure", "alarmDone", "alarmRetry"]) })
      .passthrough(),
  })
  .strict();
export type D4EventRecord = z.infer<typeof d4EventRecordSchema>;
const constructionSchema = z
  .object({
    object: d4ObjectAddressSchema,
    construction: z.uuidv7(),
    cause: z.enum(["start", "rebuild", "isolate"]),
    isolate: z.uuidv7(),
  })
  .strict();
export type D4ConstructionRecord = z.infer<typeof constructionSchema>;
export const d4StderrRecordSchema = z.union([
  z.object({ centralEvent: d4EventRecordSchema }).strict(),
  z.object({ centralConstruction: constructionSchema }).strict(),
  z
    .object({
      centralAlarmDropped: z
        .object({ object: d4ObjectAddressSchema, at: epochMillisecondsSchema })
        .strict(),
    })
    .strict(),
]);
export type D4StderrRecord = z.infer<typeof d4StderrRecordSchema>;

export function parseD4StderrLine(line: string): D4StderrRecord | undefined {
  if (Buffer.byteLength(line) > 64 * 1024 * 1024)
    throw new Error("D4 event line exceeds capture bound");
  const trimmed = line.trim();
  // Unrelated process diagnostics are retained by the caller, never treated as proof records.
  if (!/^\{\s*"central/.test(trimmed)) return undefined;
  return d4StderrRecordSchema.parse(decode(trimmed));
}
export function objectAddressKey(address: D4ObjectAddress): string {
  return address.kind === "fleet" ? "fleet" : `copy:${address.value}`;
}

export class D4EventSequence {
  private readonly constructions = new Map<string, D4ConstructionRecord>();
  private readonly next = new Map<string, bigint>();
  private readonly ids = new Set<string>();
  private readonly events: D4EventRecord[] = [];
  private readonly errors: string[] = [];
  observeLine(line: string): void {
    try {
      const record = parseD4StderrLine(line);
      if (record !== undefined) this.observe(record);
    } catch (error: unknown) {
      this.errors.push(String(error));
      throw error;
    }
  }
  observe(record: D4StderrRecord): void {
    try {
      if ("centralConstruction" in record) {
        const value = record.centralConstruction;
        const key = `${objectAddressKey(value.object)}:${value.construction}`;
        if (this.constructions.has(key)) throw new Error("duplicate construction observation");
        this.constructions.set(key, value);
        this.next.set(key, 0n);
      } else if ("centralEvent" in record) {
        const value = record.centralEvent;
        const key = `${objectAddressKey(value.object)}:${value.construction}`;
        if (!this.constructions.has(key)) throw new Error("event has no captured construction");
        if (this.next.get(key) !== value.sequence) throw new Error("event sequence gap or replay");
        if (this.ids.has(value.eventId)) throw new Error("duplicate event UUID");
        this.ids.add(value.eventId);
        this.next.set(key, value.sequence + 1n);
        this.events.push(structuredClone(value));
      }
    } catch (error: unknown) {
      this.errors.push(String(error));
      throw error;
    }
  }
  requireComplete(): void {
    if (this.errors.length > 0) throw new Error("D4 event capture has sequence errors");
    if (this.constructions.size === 0) throw new Error("no D4 construction observed");
  }
  records(): readonly D4EventRecord[] {
    return structuredClone(this.events);
  }
}

const pageSchema = z
  .object({
    kind: z.literal("events"),
    value: z
      .object({
        records: z.array(
          z.object({ cursor: unsigned64Schema, record: d4EventRecordSchema }).strict(),
        ),
        firstRetained: unsigned64Schema,
        through: unsigned64Schema,
      })
      .strict(),
  })
  .strict();
export function parseD4EventPage(input: {
  readonly address: D4ObjectAddress;
  readonly after: bigint;
  readonly response: unknown;
}): z.infer<typeof pageSchema>["value"] {
  const after = unsigned64Schema.parse(input.after);
  const page = pageSchema.parse(input.response).value;
  if (
    page.through < after ||
    page.firstRetained > page.through + 1n ||
    (page.firstRetained > 0n && after < page.firstRetained - 1n)
  )
    throw new Error("D4 event ring cursor unavailable or truncated");
  let cursor = after;
  for (const item of page.records) {
    cursor += 1n;
    if (
      item.cursor !== cursor ||
      objectAddressKey(item.record.object) !== objectAddressKey(input.address)
    )
      throw new Error("D4 event ring has a gap, replay or wrong object");
  }
  if (cursor !== page.through) throw new Error("D4 event ring omitted its covered tail");
  return page;
}

export function requireAlarmCallsFromDueUnits(
  record: D4EventRecord,
  expectedDueUnits: ReadonlySet<string>,
): void {
  if (record.event.kind !== "alarm") throw new Error("expected actual Alarm event");
  for (const call of record.calls) {
    const key = `${call.unitKind}:${call.subject}`;
    if (
      !expectedDueUnits.has(key) ||
      !record.units.some(
        (unit): boolean => unit.unitKind === call.unitKind && unit.subject === call.subject,
      )
    )
      throw new Error(`Alarm outbound call is not bound to an expected due unit: ${key}`);
  }
}
