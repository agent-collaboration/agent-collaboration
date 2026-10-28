import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { boardIdentity } from "../clients/board-requests.ts";
import type { CaseRegistration } from "../coverage.ts";
import { field } from "../exact-json.ts";
import { ObservationOpen, type Receipt } from "../receipt.ts";
import type { PreparedRun } from "../run-setup.ts";
import type { JourneyMachine } from "../run-context.ts";
import { finishAndRepeatRemoteMessage, submitRemoteMessage } from "./direct-messages.ts";

export const messageCancellationCase = {
  id: "remote-cancel-before-and-after-delivery",
  rows: ["OB4:8"],
  evidence: "journey",
} satisfies CaseRegistration;
async function settleCancellation(input: {
  source: JourneyMachine;
  item: string;
  receipt: Receipt;
  cancelId?: string;
}): Promise<{ cancel: string; outcome: unknown }> {
  const request = {
    cancelId: input.cancelId ?? boardIdentity(),
    target: input.item,
    sender: { kind: "agent", sender: input.source.recipient.target },
  };
  const deadline = Date.now() + 30_000;
  do {
    const original = await input.source.installation.mcp.call("message_cancel", request);
    const result = z
      .object({ cancel: z.string(), target: z.string(), outcome: z.object({ kind: z.string() }) })
      .parse(original);
    await input.receipt.equal(
      "cancel keeps its submitted identity",
      result.cancel,
      request.cancelId,
    );
    await input.receipt.equal("cancel keeps its original target", result.target, input.item);
    if (result.outcome.kind !== "pending") {
      const replay = await input.source.installation.mcp.call("message_cancel", request);
      await input.receipt.equal("same cancel replays its terminal outcome", replay, original);
      return { cancel: result.cancel, outcome: field(replay, "outcome") };
    }
    await setTimeout(200);
  } while (Date.now() < deadline);
  throw new ObservationOpen("cancel remains pending; the contract promises no 30-second deadline");
}
async function observeCentralCustody(source: JourneyMachine, item: string): Promise<void> {
  const deadline = Date.now() + 30_000;
  do {
    const shown = await source.installation.mcp.call("router_show", {
      caller: source.recipient.target,
      reference: item,
    });
    const kind = field(shown, "mail", "status", "kind");
    if (kind === "sent") return;
    if (kind !== "queued")
      throw new Error(
        "offline recipient original reached an unexpected outcome before cancellation",
      );
    await setTimeout(200);
  } while (Date.now() < deadline);
  throw new ObservationOpen("original has no observed central custody before cancellation");
}
export async function messageCancellation(context: PreparedRun, receipt: Receipt): Promise<void> {
  const [source, target] = context.machines;
  const gate = context.gates.get(target.installation.machine.name);
  if (gate === undefined) throw new ObservationOpen("recipient Session gate is unavailable");
  const input = {
    sender: source.installation,
    senderSession: source.recipient.target,
    recipient: target.recipient,
    receipt,
  };
  const connected = await gate.ready();
  const before = target.recipient.position();
  const cancelId = boardIdentity();
  let prevented;
  gate.pause();
  try {
    prevented = await submitRemoteMessage(input);
    await observeCentralCustody(source, prevented.admission.item);
    const pending = z
      .object({
        cancel: z.literal(cancelId),
        target: z.literal(prevented.admission.item),
        outcome: z.object({ kind: z.literal("pending") }),
      })
      .parse(
        await source.installation.mcp.call("message_cancel", {
          cancelId,
          target: prevented.admission.item,
          sender: { kind: "agent", sender: source.recipient.target },
        }),
      );
    await receipt.equal(
      "cancel waits for the disconnected recipient's injection decision",
      pending.outcome,
      { kind: "pending" },
    );
    await target.recipient.absent(null, 1000, before);
  } finally {
    gate.resume();
  }
  await gate.ready(connected.welcomes);
  if (prevented === undefined) throw new ObservationOpen("no cancellable original was admitted");
  const prevention = await settleCancellation({
    source,
    item: prevented.admission.item,
    receipt,
    cancelId,
  });
  await receipt.equal("recipient prevents the original before model input", prevention.outcome, {
    kind: "prevented",
  });
  await source.installation.mcp.call("message_send", {
    ...prevented.request,
    itemId: prevented.admission.item,
  });
  await target.recipient.absent(null, 3000, before);
  await receipt.equal(
    "original replay cannot undo the winning cancel",
    field(
      await source.installation.mcp.call("router_show", {
        caller: source.recipient.target,
        reference: prevented.admission.item,
      }),
      "mail",
      "status",
    ),
    { kind: "cancelled" },
  );
  const delivered = await submitRemoteMessage(input);
  await finishAndRepeatRemoteMessage(input, delivered);
  const beforeLateCancel = target.recipient.position();
  const result = await settleCancellation({ source, item: delivered.admission.item, receipt });
  await receipt.equal("cancel after actual model input is too late", result.outcome, {
    kind: "tooLate",
    at: "delivered",
  });
  const shown = await target.installation.mcp.call("router_show", {
    caller: target.recipient.target,
    reference: delivered.admission.item,
  });
  const mark = z
    .object({ cancel: z.string(), tooLateAt: z.literal("delivered") })
    .parse(field(shown, "record", "cancelledAfterDelivery"));
  await receipt.equal("recipient marks the same delivered input cancelled", mark, {
    cancel: result.cancel,
    tooLateAt: "delivered",
  });
  await Promise.all([
    target.recipient.unique(delivered.admission.item, 3000),
    target.recipient.absent(null, 3000, beforeLateCancel),
  ]);
  await target.recipient.unique(delivered.admission.item);
}
