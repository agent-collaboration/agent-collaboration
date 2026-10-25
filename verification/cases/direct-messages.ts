import { randomUUID, createHash } from "node:crypto";
import { z } from "zod";
import { setTimeout } from "node:timers/promises";
import { sessionSchema, type Session } from "../declaration.ts";
import type { Installation } from "../machines/installation.ts";
import type { CodexSession } from "../recipients/codex-session.ts";
import type { Receipt } from "../receipt.ts";
import { ObservationOpen } from "../receipt.ts";
import { field } from "../exact-json.ts";

const queuedSchema = z.object({ queued: z.object({ item: z.string(), target: sessionSchema }) });
export interface MessageParticipants {
  readonly sender: Installation;
  readonly senderSession: Session;
  readonly recipient: CodexSession;
  readonly receipt: Receipt;
}
async function deliveredStatus(input: MessageParticipants, item: string): Promise<unknown> {
  const deadline = Date.now() + 30_000;
  do {
    const reply = await input.sender.mcp.call("router_show", {
      caller: input.senderSession,
      reference: item,
    });
    const status = field(reply, "mail", "status");
    if (z.object({ kind: z.literal("delivered") }).safeParse(status).success) return status;
    await setTimeout(200);
  } while (Date.now() < deadline);
  throw new ObservationOpen("sender has no observed terminal delivered status");
}
interface Submission {
  readonly admission: z.infer<typeof queuedSchema>["queued"];
  readonly request: Readonly<Record<string, unknown>>;
  readonly marker: string;
}
export async function submitRemoteMessage(input: MessageParticipants): Promise<Submission> {
  await input.receipt.require(
    "OB4 message crosses installations",
    input.senderSession.endpoint.serviceId !== input.recipient.target.endpoint.serviceId,
  );
  const marker = `verification-message-${randomUUID()}`;
  const request = {
    target: input.recipient.target,
    delivery: "auto",
    message: {
      kind: "agent",
      sender: input.senderSession,
      text: `${marker}\nReply exactly the first line. Do not use tools, read files, delegate or change anything.`,
    },
  };
  const initial = queuedSchema.parse(await input.sender.mcp.call("message_send", request));
  await input.receipt.equal(
    "OB4 remote sender outbox keeps recipient",
    initial.queued.target,
    input.recipient.target,
  );
  return { admission: initial.queued, request, marker };
}
export async function finishAndRepeatRemoteMessage(
  input: MessageParticipants,
  submission: Submission,
): Promise<void> {
  const { admission, request, marker } = submission;
  const delivery = await input.recipient.completed(admission.item, marker);
  const finalStatus = await deliveredStatus(input, admission.item);
  const repeated = queuedSchema.parse(
    await input.sender.mcp.call("message_send", { ...request, itemId: admission.item }),
  );
  await input.receipt.equal(
    "OB4 duplicate sender hop keeps item identity",
    repeated.queued,
    admission,
  );
  await input.recipient.unique(admission.item, 3000);
  await input.receipt.equal(
    "same-ID replay preserves final sender status",
    await deliveredStatus(input, admission.item),
    finalStatus,
  );
  await input.recipient.unique(admission.item);
  await input.receipt.record("remoteMessageAndReplay", {
    itemSHA256: createHash("sha256").update(admission.item).digest("hex"),
    turnSHA256: createHash("sha256").update(delivery.turnId).digest("hex"),
    agentTextSHA256: delivery.agentTextSHA256,
    exactlyOnceWithinQuietWindow: true,
    quietWindowMilliseconds: 3000,
  });
}
export async function sendAndRepeatRemoteMessage(input: MessageParticipants): Promise<void> {
  await finishAndRepeatRemoteMessage(input, await submitRemoteMessage(input));
}
