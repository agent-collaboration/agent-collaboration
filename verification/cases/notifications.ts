import { setTimeout } from "node:timers/promises";
import { isDeepStrictEqual } from "node:util";
import { z } from "zod";
import { identitySchema, sessionSchema, type Identity } from "../declaration.ts";
import {
  acceptedWrite,
  boardIdentity,
  projectRecords,
  projectPlacementSchema,
  feedAtLeast,
  type FeedCursor,
  writeHeader,
} from "../clients/board-requests.ts";
import type { Installation } from "../machines/installation.ts";
import type { JourneyMachine } from "../run-context.ts";
import { ObservationOpen, type Receipt } from "../receipt.ts";

const intentSchema = z.object({
  kind: z.literal("notifyIntent"),
  record: z.object({
    envelope: z.object({ position: projectPlacementSchema }),
    intentId: z.string(),
    cause: z.object({ id: z.string() }),
    recipient: z.object({
      kind: z.literal("resolved"),
      recipient: z.object({ actor: identitySchema }),
    }),
    state: z.discriminatedUnion("kind", [
      z.object({ kind: z.literal("emitted") }),
      z.object({ kind: z.literal("handedOff"), item: z.string() }),
      z.object({ kind: z.literal("superseded") }),
      z.object({ kind: z.literal("refused"), reason: z.unknown() }),
    ]),
  }),
});
const causeSchema = intentSchema.extend({
  record: intentSchema.shape.record.pick({ cause: true }),
});
export interface NotificationInput {
  readonly source: Installation;
  readonly actor: Identity;
  readonly project: string;
  readonly recipients: readonly JourneyMachine[];
  readonly receipt: Receipt;
}
async function frozenItems(
  input: NotificationInput,
  post: string,
): Promise<{ items: readonly string[]; intents: readonly string[]; through: FeedCursor }> {
  const deadline = Date.now() + 30_000;
  do {
    const records = (
      await projectRecords({
        client: input.source.mcp,
        project: input.project,
        tool: "intent_list",
      })
    )
      .filter((record): boolean => causeSchema.parse(record).record.cause.id === post)
      .map((record) => intentSchema.parse(record).record);
    if (records.length > input.recipients.length) throw new Error("extra notification recipient");
    if (
      records.some(
        (record): boolean => record.state.kind === "superseded" || record.state.kind === "refused",
      )
    )
      throw new Error("notification did not retain its handoff");
    if (
      records.length === input.recipients.length &&
      records.every((record): boolean => record.state.kind === "handedOff")
    ) {
      const handoffs = input.recipients.map((recipient) => {
        const matches = records.filter((record): boolean =>
          isDeepStrictEqual(record.recipient.recipient.actor, {
            kind: "session",
            session: sessionSchema.parse(recipient.recipient.target),
          }),
        );
        const match = matches[0];
        if (matches.length !== 1 || match?.state.kind !== "handedOff")
          throw new Error("frozen handoff does not name exactly one actual recipient");
        return { item: match.state.item, intent: match.intentId };
      });
      const cursors = records.map((record): FeedCursor => record.envelope.position.position.cursor);
      const first = cursors[0];
      if (first === undefined) throw new Error("notification has no recorded handoff cursor");
      const through = cursors.reduce(
        (latest, cursor): FeedCursor => (feedAtLeast(cursor, latest) ? cursor : latest),
        first,
      );
      return {
        items: handoffs.map((row) => row.item),
        intents: handoffs.map((row) => row.intent),
        through,
      };
    }
    await setTimeout(200);
  } while (Date.now() < deadline);
  throw new ObservationOpen("notification handoffs did not settle");
}
export interface FrozenNotification {
  readonly request: Readonly<Record<string, unknown>>;
  readonly decision: unknown;
  readonly post: string;
  readonly items: readonly string[];
  readonly intents: readonly string[];
  readonly marker: string;
  readonly through: FeedCursor;
}
export async function freezeNotification(input: NotificationInput): Promise<FrozenNotification> {
  if (input.recipients.length === 0) throw new Error("notification requires real recipients");
  const post = boardIdentity();
  const marker = `verification-notification-${post}`;
  const request = {
    ...writeHeader(input.actor),
    projectId: input.project,
    postId: post,
    about: { kind: "project", subject: { line: "Release verification notification" } },
    text: `${marker}\nAcknowledge this notification. Do not use tools, delegate, read files or change anything.`,
    references: [],
    role: null,
    watch: false,
    addressees: input.recipients.map((recipient) => ({
      kind: "actor",
      recipient: { kind: "session", session: recipient.recipient.target },
    })),
  };
  const decision = await acceptedWrite({
    client: input.source.mcp,
    tool: "thread_create",
    request,
  });
  const { items, intents, through } = await frozenItems(input, post);
  await input.receipt.equal(
    "one cause has a distinct item per recipient",
    new Set(items).size,
    input.recipients.length,
  );
  return { request, decision, post, items, intents, marker, through };
}
export async function finishNotification(
  input: NotificationInput,
  frozen: FrozenNotification,
): Promise<void> {
  const { request, decision, post, items, intents } = frozen;
  for (const [index, machine] of input.recipients.entries()) {
    const recipient = machine.recipient;
    const item = z.string().parse(items[index]);
    await recipient.completed(item, [
      "📋 Router: board notification",
      "New thread post",
      `router://${recipient.target.endpoint.serviceId}/push/${item}`,
    ]);
    const shown = await machine.installation.mcp.call("router_show", {
      caller: recipient.target,
      reference: item,
    });
    const record = z
      .object({
        record: z.object({
          kind: z.literal("board-notification"),
          target: sessionSchema,
          body: z.null(),
          headerFacts: z.object({
            kind: z.literal("boardNotification"),
            project: z.string(),
            intent: z.string(),
            cause: z.object({ kind: z.literal("post"), id: z.string() }),
            summary: z.literal("New thread post"),
          }),
        }),
      })
      .parse(shown).record;
    await input.receipt.equal(
      "native notification retains the actual recipient and cause pointer",
      {
        target: record.target,
        project: record.headerFacts.project,
        cause: record.headerFacts.cause,
        intent: record.headerFacts.intent,
      },
      {
        target: recipient.target,
        project: input.project,
        cause: { kind: "post", id: post },
        intent: z.string().parse(intents[index]),
      },
    );
  }
  await input.receipt.equal(
    "notification cause replay preserves its decision",
    await acceptedWrite({ client: input.source.mcp, tool: "thread_create", request }),
    decision,
  );
  await input.receipt.equal(
    "notification cause replay preserves original handoffs",
    await frozenItems(input, post),
    { items, intents, through: frozen.through },
  );
  await Promise.all(
    input.recipients.map((recipient, index): Promise<void> =>
      recipient.recipient.unique(z.string().parse(items[index]), 3000),
    ),
  );
  for (const [index, recipient] of input.recipients.entries())
    await recipient.recipient.unique(z.string().parse(items[index]));
}
export async function notifyAndRepeat(input: NotificationInput): Promise<void> {
  await finishNotification(input, await freezeNotification(input));
}
