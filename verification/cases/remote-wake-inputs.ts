import { z } from "zod";
import { boardIdentity } from "../clients/board-requests.ts";
import type { RouterMcp } from "../clients/router-mcp.ts";
import { sessionSchema } from "../declaration.ts";
import { field } from "../exact-json.ts";
import { ObservationOpen } from "../receipt.ts";
import type { JourneyMachine } from "../run-context.ts";

export const wakeSnapshotSchema = z.object({
  definition: z.object({
    wakeupId: z.string(),
    createdBy: z.unknown(),
    message: z.object({ target: sessionSchema, content: z.unknown(), delivery: z.string() }),
    timing: z.unknown(),
    anchorAt: z.iso.datetime(),
    expiresAt: z.iso.datetime().nullable(),
  }),
  state: z.enum(["active", "paused", "cancelled", "expired", "finished"]),
  firstFire: z
    .object({ wakeupId: z.string(), occurrenceId: z.string(), dueAt: z.iso.datetime() })
    .nullable(),
});
export function wakeRequest(input: {
  source: JourneyMachine;
  target: JourneyMachine;
  timing: Readonly<Record<string, unknown>>;
  expiry: Readonly<Record<string, unknown>>;
}): Readonly<Record<string, unknown>> {
  const operationId = boardIdentity();
  return {
    operationId,
    requestedBy: { kind: "session", ...input.source.recipient.target },
    replace: false,
    message: {
      target: input.target.recipient.target,
      delivery: "auto",
      content: {
        kind: "agent",
        sender: input.source.recipient.target,
        text: `verification-wake-${operationId}\nReply exactly the first line. Do not use tools, read files, delegate or change anything.`,
      },
    },
    timing: input.timing,
    expiry: input.expiry,
  };
}
export async function wakeRefusal(
  client: RouterMcp,
  request: Readonly<Record<string, unknown>>,
): Promise<unknown> {
  const result = await client.outcome("wake_send", request);
  if (result.kind === "refused") return result.value;
  if (result.kind === "protocolRefusal") return field(result.error, "data");
  if (result.kind === "unconfirmed") throw new ObservationOpen("wake refusal was unconfirmed");
  if (result.kind === "answered") {
    const admitted = z
      .object({ definition: z.object({ wakeupId: z.string() }) })
      .parse(result.value);
    const failures: unknown[] = [new Error("invalid wake was admitted instead of refused")];
    try {
      await client.call("wake_cancel", {
        operationId: boardIdentity(),
        wakeupId: admitted.definition.wakeupId,
      });
    } catch (error: unknown) {
      failures.push(error);
    }
    throw new AggregateError(failures, "unexpected wake admission or cleanup failed");
  }
  throw new Error("wake request did not return a typed refusal");
}
export async function wakeRecords(client: RouterMcp): Promise<readonly unknown[]> {
  const page = z
    .object({ records: z.array(z.unknown()), nextCursor: z.string().nullable() })
    .parse(await client.call("wake_list", { cursor: null, limit: 100n }));
  if (page.nextCursor !== null) throw new ObservationOpen("wake inventory exceeded one full page");
  return page.records;
}
export async function cancelKnownWakes(
  client: RouterMcp,
  wakeups: ReadonlySet<string>,
): Promise<readonly unknown[]> {
  const results = await Promise.allSettled(
    [...wakeups].map((wakeupId): Promise<unknown> =>
      client.call("wake_cancel", { operationId: boardIdentity(), wakeupId }),
    ),
  );
  return results.flatMap((result): unknown[] =>
    result.status === "rejected" ? [result.reason] : [],
  );
}
