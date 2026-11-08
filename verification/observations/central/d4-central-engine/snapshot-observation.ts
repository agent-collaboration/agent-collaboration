/** S8/S10 views from draft contract v3.2; raw responses remain the authoritative evidence. */
import { z } from "zod";
import {
  epochMillisecondsSchema,
  unsigned64Schema,
  type D4ObjectAddress,
} from "./event-observation.ts";

const label = z.string().min(1);
const attemptSchema = z
  .object({
    id: z.uuidv7(),
    subject: z.uuidv7(),
    kind: label,
    state: z.enum(["waiting", "in_flight", "given_up"]),
    sendId: z.uuidv7().nullable(),
    connectionId: z.uuidv7().nullable(),
    sessionId: z.uuidv7().nullable(),
    sentVersion: unsigned64Schema.nullable(),
    nextAttemptAt: epochMillisecondsSchema.nullable(),
    attempts: unsigned64Schema,
    firstFailedAt: epochMillisecondsSchema.nullable(),
    lastErrorClass: label.nullable(),
    lastError: label.nullable(),
    createdAt: epochMillisecondsSchema,
    updatedAt: epochMillisecondsSchema,
  })
  .strict()
  .superRefine((row, context): void => {
    if (
      row.state !== "in_flight" &&
      [row.sendId, row.connectionId, row.sessionId, row.sentVersion].some(
        (value): boolean => value !== null,
      )
    )
      context.addIssue({
        code: "custom",
        message: "inactive attempt carries a current send binding",
      });
    if (row.state === "given_up" && row.nextAttemptAt !== null)
      context.addIssue({ code: "custom", message: "given_up attempt is still scheduled" });
    if (
      row.state === "in_flight" &&
      ["push", "status"].includes(row.kind) &&
      (row.sendId === null ||
        row.connectionId === null ||
        row.sessionId === null ||
        row.nextAttemptAt === null)
    )
      context.addIssue({
        code: "custom",
        message: "in_flight socket attempt lacks send/session/deadline",
      });
    if (row.kind === "status" && row.state === "in_flight" && row.sentVersion === null)
      context.addIssue({ code: "custom", message: "in_flight status lacks sentVersion" });
    if (row.kind !== "status" && row.sentVersion !== null)
      context.addIssue({ code: "custom", message: "non-status attempt has sentVersion" });
  });
const feedSchema = z
  .object({
    head: unsigned64Schema,
    retainedThrough: unsigned64Schema,
    effectiveFloor: unsigned64Schema,
  })
  .strict()
  .refine(
    (feed): boolean =>
      feed.retainedThrough <= feed.effectiveFloor && feed.effectiveFloor <= feed.head,
    "invalid effective feed floor",
  );
export const d4SnapshotResponseSchema = z
  .object({
    kind: z.literal("snapshot"),
    value: z
      .object({
        attempts: z.array(attemptSchema),
        sessions: z.array(
          z
            .object({
              session: z.uuidv7(),
              peer: z.uuidv7(),
              connection: z.uuidv7(),
              registration: z.enum(["opening", "attaching", "attached", "closed"]),
            })
            .strict(),
        ),
        inventory: z.array(z.uuidv7()),
        windows: z.array(z.object({ connection: z.uuidv7(), inFlight: unsigned64Schema }).strict()),
        noticeProgress: z.array(
          z
            .object({
              machine: z.uuidv7(),
              lastIssued: unsigned64Schema,
              expiredThrough: unsigned64Schema,
              appliedThrough: unsigned64Schema.nullable(),
            })
            .strict(),
        ),
        feed: feedSchema.nullable(),
        owners: z
          .object({
            outbox: z.array(z.object({ item: z.uuidv7(), custody: label }).strict()),
            statusOwed: z.array(
              z.object({ id: z.uuidv7(), item: z.uuidv7(), version: unsigned64Schema }).strict(),
            ),
          })
          .strict(),
      })
      .strict()
      .superRefine((snapshot, context): void => {
        if (
          new Set(snapshot.attempts.map((row): string => row.id)).size !==
            snapshot.attempts.length ||
          new Set(snapshot.attempts.map((row): string => `${row.kind}:${row.subject}`)).size !==
            snapshot.attempts.length
        )
          context.addIssue({ code: "custom", message: "duplicate attempt id or kind/subject" });
        if (
          new Set(snapshot.inventory).size !== snapshot.inventory.length ||
          new Set(snapshot.windows.map((row): string => row.connection)).size !==
            snapshot.windows.length
        )
          context.addIssue({ code: "custom", message: "duplicate inventory or window connection" });
        const current = snapshot.sessions.filter(
          (session): boolean =>
            session.registration === "attached" && snapshot.inventory.includes(session.connection),
        );
        if (new Set(current.map((session): string => session.connection)).size !== current.length)
          context.addIssue({ code: "custom", message: "current socket has ambiguous sessions" });
      }),
  })
  .strict();
export type D4Snapshot = z.infer<typeof d4SnapshotResponseSchema>["value"];

export function parseD4Snapshot(address: D4ObjectAddress, response: unknown): D4Snapshot {
  const snapshot = d4SnapshotResponseSchema.parse(response).value;
  if (address.kind === "fleet" && snapshot.feed === null)
    throw new Error("Fleet snapshot lacks its feed");
  if (
    address.kind === "copy" &&
    (snapshot.feed !== null ||
      snapshot.noticeProgress.length > 0 ||
      snapshot.sessions.length > 0 ||
      snapshot.inventory.length > 0 ||
      snapshot.windows.length > 0 ||
      snapshot.owners.outbox.length > 0 ||
      snapshot.owners.statusOwed.length > 0)
  )
    throw new Error("Copy snapshot contains Fleet-only state");
  return snapshot;
}

export function requireWindowSnapshotAgreement(
  snapshot: D4Snapshot,
  wireCurrent: ReadonlyMap<string, bigint>,
): void {
  const current = new Map(
    snapshot.sessions
      .filter(
        (session): boolean =>
          session.registration === "attached" && snapshot.inventory.includes(session.connection),
      )
      .map((session): [string, string] => [session.connection, session.session]),
  );
  const expected = new Map<string, bigint>();
  for (const row of snapshot.attempts) {
    if (
      row.state !== "in_flight" ||
      !["push", "status"].includes(row.kind) ||
      row.connectionId === null ||
      row.sessionId === null
    )
      continue;
    if (current.get(row.connectionId) !== row.sessionId) continue;
    expected.set(row.connectionId, (expected.get(row.connectionId) ?? 0n) + 1n);
  }
  const reported = new Map(
    snapshot.windows.map((row): [string, bigint] => [row.connection, row.inFlight]),
  );
  for (const [connection, count] of wireCurrent) {
    z.uuidv7().parse(connection);
    unsigned64Schema.parse(count);
  }
  const connections = new Set([
    ...current.keys(),
    ...expected.keys(),
    ...reported.keys(),
    ...wireCurrent.keys(),
  ]);
  for (const connection of connections) {
    const count = expected.get(connection) ?? 0n;
    if (
      count > 32n ||
      (reported.get(connection) ?? 0n) !== count ||
      (wireCurrent.get(connection) ?? 0n) !== count
    )
      throw new Error(`window mismatch or exceeds 32 on ${connection}`);
  }
}

export const d4LongOperationsResponseSchema = z
  .object({
    kind: z.literal("longOperations"),
    value: z
      .object({
        registeredKinds: z.array(label),
        drives: z.array(
          z
            .object({
              operation: z.uuidv7(),
              by: z.enum(["driveOne", "dueBatch", "standIn"]),
              eventId: z.uuidv7(),
            })
            .strict(),
        ),
        storedHolderReplies: z.array(
          z
            .object({
              call: z.object({ operation: z.uuidv7(), step: label }).strict(),
              target: label,
              stored: z.boolean(),
            })
            .strict(),
        ),
      })
      .strict(),
  })
  .strict();
export type D4LongOperations = z.infer<typeof d4LongOperationsResponseSchema>["value"];
const centralLongOperationKinds = [
  "fleet-sync.register",
  "fleet-sync.import",
  "fleet-sync.retire",
  "fleet-sync.holder-change",
  "fleet-sync.holder-change-cancel",
];
export function requireCentralLongOperations(
  observed: D4LongOperations,
  requestOperations: readonly string[],
): void {
  if (
    observed.registeredKinds.length !== centralLongOperationKinds.length ||
    new Set(observed.registeredKinds).size !== centralLongOperationKinds.length ||
    centralLongOperationKinds.some((kind): boolean => !observed.registeredKinds.includes(kind))
  )
    throw new Error("central does not register exactly its five U6 kinds");
  if (requestOperations.length === 0) throw new Error("no actual request operation declared");
  for (const operation of requestOperations) {
    z.uuidv7().parse(operation);
    if (
      !observed.drives.some(
        (drive): boolean => drive.operation === operation && drive.by === "driveOne",
      ) ||
      observed.drives.some(
        (drive): boolean => drive.operation === operation && drive.by !== "driveOne",
      )
    )
      throw new Error("request operation was not driven only by actual drive_one");
  }
}
