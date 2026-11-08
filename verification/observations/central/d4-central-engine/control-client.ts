/** Exact adjacent-tagged D4 draft-v3 controls over the existing owner-authenticated route.
 * Construct only for a declared local target. Passivity must be observed separately before use.
 */
import { z } from "zod";
import { setTimeout } from "node:timers/promises";
import { CentralOwner } from "../../../clients/central-owner.ts";
import {
  parseD4Snapshot,
  d4LongOperationsResponseSchema,
  type D4Snapshot,
  type D4LongOperations,
} from "./snapshot-observation.ts";
import {
  d4ObjectAddressSchema,
  epochMillisecondsSchema,
  unsigned64Schema,
  parseD4EventPage,
  type D4ObjectAddress,
} from "./event-observation.ts";

const source = z.enum([
  "attempt_retries",
  "notice_expiry",
  "mail_retention",
  "forward_expiry",
  "feed_retention",
  "staged_part_expiry",
  "long_operations",
]);
export const d4FaultSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("nextDue"), value: z.object({ source }).strict() }).strict(),
  z.object({ kind: z.literal("lowerAlarmWrite") }).strict(),
  z
    .object({
      kind: z.literal("exactAlarmWrite"),
      value: z.object({ on: z.enum(["frame", "call", "alarm"]) }).strict(),
    })
    .strict(),
  z.object({ kind: z.literal("attemptsWrite") }).strict(),
  z.object({ kind: z.literal("cleanup"), value: z.object({ source }).strict() }).strict(),
  z
    .object({
      kind: z.literal("physicalClose"),
      value: z.object({ connection: z.uuidv7() }).strict(),
    })
    .strict(),
]);
export type D4Fault = z.infer<typeof d4FaultSchema>;
export const d4FixtureSeedSchema = z.discriminatedUnion("kind", [
  z
    .object({
      kind: z.literal("corruptNotice"),
      value: z.object({ machine: z.uuidv7(), position: unsigned64Schema }).strict(),
    })
    .strict(),
  z
    .object({
      kind: z.literal("corruptFeedRow"),
      value: z.object({ position: unsigned64Schema }).strict(),
    })
    .strict(),
  z
    .object({
      kind: z.literal("staleAlarm"),
      value: z.object({ at: epochMillisecondsSchema }).strict(),
    })
    .strict(),
  z
    .object({
      kind: z.literal("noticeLog"),
      value: z
        .object({
          machine: z.uuidv7(),
          clearedThrough: unsigned64Schema,
          positions: z.array(unsigned64Schema).min(1),
          expiresAt: epochMillisecondsSchema,
        })
        .strict(),
    })
    .strict(),
  z
    .object({
      kind: z.literal("agedAttempt"),
      value: z
        .object({
          subject: z.uuidv7(),
          attemptKind: z.string().min(1),
          attempts: unsigned64Schema,
          firstFailedAt: epochMillisecondsSchema,
        })
        .strict(),
    })
    .strict(),
]);
export type D4FixtureSeed = z.infer<typeof d4FixtureSeedSchema>;
const doneSchema = z.object({ kind: z.literal("done") }).strict();
const faultObservation = z
  .object({ armed: unsigned64Schema, reached: unsigned64Schema, consumed: unsigned64Schema })
  .strict();
const barrierPointSchema = z.enum([
  "beforeRecomputeTx",
  "afterUnitCommit",
  "afterStepLower",
  "beforeClaim",
]);
export type D4BarrierPoint = z.infer<typeof barrierPointSchema>;
const corruptRowSchema = z
  .object({
    table: z.string().min(1).max(256),
    key: z.string().min(1).max(256),
    field: z.string().min(1).max(256),
  })
  .strict();
export type D4CorruptRow = z.infer<typeof corruptRowSchema>;

export class D4ControlFailure extends Error {
  readonly failure: string;
  readonly corruptRow: D4CorruptRow | undefined;
  constructor(failure: string, corruptRow?: D4CorruptRow) {
    super(
      `D4 control refused: ${failure}${corruptRow === undefined ? "" : ` (${corruptRow.table}.${corruptRow.field} of ${corruptRow.key})`}`,
    );
    this.failure = failure;
    this.corruptRow = corruptRow === undefined ? undefined : { ...corruptRow };
  }
}

export class D4ControlClient {
  private readonly owner: CentralOwner;
  private readonly address: D4ObjectAddress;
  private readonly shell: "tokio" | "workerd";
  private readonly passivity: "unqualified" | "observed";
  private readonly clockPassivity: "unqualified" | "observed";
  constructor(input: {
    readonly owner: CentralOwner;
    readonly address: D4ObjectAddress;
    readonly shell: "tokio" | "workerd";
    readonly passivity: "unqualified" | "observed";
    readonly clockPassivity: "unqualified" | "observed";
  }) {
    if (
      input.owner.origin.protocol !== "http:" ||
      input.owner.origin.hostname !== "127.0.0.1" ||
      Number(input.owner.origin.port) < 1024 ||
      [8787, 8788, 19741, 19742, 19743, 43127].includes(Number(input.owner.origin.port))
    )
      throw new Error("D4 controls require declared isolated local central");
    this.owner = input.owner;
    this.address = d4ObjectAddressSchema.parse(input.address);
    this.shell = input.shell;
    this.passivity = input.passivity;
    this.clockPassivity = input.clockPassivity;
  }
  private requireTokio(): void {
    if (this.shell !== "tokio") throw new Error("Tokio-only D4 control");
  }
  private async control(command: unknown): Promise<unknown> {
    if (this.passivity !== "observed") throw new Error("D4 control passivity is unqualified");
    const response = await this.owner.response({
      path: "/admin/v1/conformance/runtime",
      method: "POST",
      body: { address: this.address, command },
      // Producer causal waits end at 30 s. Leave bounded transport time for its
      // named 409 response instead of aborting before it can reach this caller.
      timeoutMilliseconds: 35_000,
    });
    if (response.status === 409) {
      if (typeof response.body === "string") {
        const failure = z.string().min(1).max(256).parse(response.body);
        if (failure === "corruptRow")
          throw new Error("corruptRow refusal lacks table, key and field");
        throw new D4ControlFailure(failure);
      }
      const refusal = z.object({ corruptRow: corruptRowSchema }).strict().parse(response.body);
      throw new D4ControlFailure("corruptRow", refusal.corruptRow);
    }
    if (response.status !== 200) throw new Error(`D4 control HTTP status ${response.status}`);
    return response.body;
  }
  async readAlarm(): Promise<bigint | null> {
    return z
      .object({
        kind: z.literal("alarm"),
        value: z.object({ stored: epochMillisecondsSchema.nullable() }).strict(),
      })
      .strict()
      .parse(await this.control({ kind: "readAlarm" })).value.stored;
  }
  async alarmDelivery(enabled: boolean): Promise<void> {
    doneSchema.parse(
      await this.control({ kind: "alarmDelivery", value: { enabled: z.boolean().parse(enabled) } }),
    );
  }
  async duplicateAlarm(): Promise<void> {
    this.requireTokio();
    doneSchema.parse(await this.control({ kind: "duplicateAlarm" }));
  }
  async settleClock(now: string): Promise<bigint> {
    this.requireTokio();
    if (this.clockPassivity !== "observed")
      throw new Error("D4 clock drain/passivity is unqualified");
    // RegistryTime is transparently serialized DateTime<Utc>, not an epoch integer.
    // Draft v3.4: the alarm loop acknowledges this clock epoch only after any
    // caused Alarm events finish and their S1 records reach the ring.
    return z
      .object({
        kind: z.literal("clockSettled"),
        value: z.object({ alarmEvents: unsigned64Schema }).strict(),
      })
      .strict()
      .parse(
        await this.control({ kind: "settleClock", value: { now: z.iso.datetime().parse(now) } }),
      ).value.alarmEvents;
  }
  async rebuildAtQuiescence(enabled: boolean): Promise<void> {
    this.requireTokio();
    doneSchema.parse(
      await this.control({
        kind: "rebuildAtQuiescence",
        value: { enabled: z.boolean().parse(enabled) },
      }),
    );
  }
  async readPending(): Promise<{ tasks: bigint; deadlines: bigint; waiters: bigint }> {
    this.requireTokio();
    return z
      .object({
        kind: z.literal("pending"),
        value: z
          .object({
            tasks: unsigned64Schema,
            deadlines: unsigned64Schema,
            waiters: unsigned64Schema,
          })
          .strict(),
      })
      .strict()
      .parse(await this.control({ kind: "readPending" })).value;
  }
  async readEvents(after: bigint): Promise<ReturnType<typeof parseD4EventPage>> {
    this.requireTokio();
    const cursor = unsigned64Schema.parse(after);
    return parseD4EventPage({
      address: this.address,
      after: cursor,
      response: await this.control({ kind: "readEvents", value: { after: cursor } }),
    });
  }
  async armFault(fault: D4Fault, count: bigint): Promise<string> {
    const value = {
      fault: d4FaultSchema.parse(fault),
      count: z.bigint().min(1n).max(4294967295n).parse(count),
    };
    return z
      .object({ kind: z.literal("faultArmed"), value: z.object({ faultId: z.uuidv7() }).strict() })
      .strict()
      .parse(await this.control({ kind: "armFault", value })).value.faultId;
  }
  async readFault(faultId: string): Promise<z.infer<typeof faultObservation>> {
    return z
      .object({ kind: z.literal("fault"), value: faultObservation })
      .strict()
      .parse(
        await this.control({ kind: "readFault", value: { faultId: z.uuidv7().parse(faultId) } }),
      ).value;
  }
  async seedFixture(seed: D4FixtureSeed): Promise<void> {
    // Explicit fixture mutation, including stale alarm setup; never part of a quiet interval.
    doneSchema.parse(
      await this.control({ kind: "seed", value: { seed: d4FixtureSeedSchema.parse(seed) } }),
    );
  }
  async snapshot(): Promise<D4Snapshot> {
    for (let attempt = 0; ; attempt += 1) {
      try {
        return parseD4Snapshot(this.address, await this.control({ kind: "snapshot" }));
      } catch (error: unknown) {
        if (
          !(error instanceof D4ControlFailure) ||
          error.failure !== "snapshotUnstable" ||
          attempt >= 2
        )
          throw error;
        // This confirmed read-only refusal is the sole retry authorized by v3.6.
        await setTimeout(25);
      }
    }
  }
  async readLongOperations(): Promise<D4LongOperations> {
    return d4LongOperationsResponseSchema.parse(await this.control({ kind: "readLongOperations" }))
      .value;
  }
  async holdAfterCommit(attemptKind: string, subject: string | null): Promise<string> {
    return z
      .object({ kind: z.literal("cutArmed"), value: z.object({ cutId: z.uuidv7() }).strict() })
      .strict()
      .parse(
        await this.control({
          kind: "holdAfterCommit",
          value: {
            attemptKind: z.string().min(1).parse(attemptKind),
            subject: z.uuidv7().nullable().parse(subject),
          },
        }),
      ).value.cutId;
  }
  async readCut(cutId: string): Promise<boolean> {
    return z
      .object({ kind: z.literal("cut"), value: z.object({ reached: z.boolean() }).strict() })
      .strict()
      .parse(await this.control({ kind: "readCut", value: { cutId: z.uuidv7().parse(cutId) } }))
      .value.reached;
  }
  async releaseCut(cutId: string): Promise<void> {
    doneSchema.parse(
      await this.control({ kind: "releaseCut", value: { cutId: z.uuidv7().parse(cutId) } }),
    );
  }
  async barrier(point: D4BarrierPoint, unitKind: string | null): Promise<string> {
    this.requireTokio();
    return z
      .object({
        kind: z.literal("barrierArmed"),
        value: z.object({ barrierId: z.uuidv7() }).strict(),
      })
      .strict()
      .parse(
        await this.control({
          kind: "barrier",
          value: {
            point: barrierPointSchema.parse(point),
            unitKind: z.string().min(1).nullable().parse(unitKind),
          },
        }),
      ).value.barrierId;
  }
  async awaitBarrier(barrierId: string): Promise<string> {
    this.requireTokio();
    return z
      .object({
        kind: z.literal("barrier"),
        value: z.object({ reached: z.literal(true), eventId: z.uuidv7() }).strict(),
      })
      .strict()
      .parse(
        await this.control({
          kind: "awaitBarrier",
          value: { barrierId: z.uuidv7().parse(barrierId) },
        }),
      ).value.eventId;
  }
  async releaseBarrier(barrierId: string): Promise<void> {
    this.requireTokio();
    doneSchema.parse(
      await this.control({
        kind: "releaseBarrier",
        value: { barrierId: z.uuidv7().parse(barrierId) },
      }),
    );
  }
}
