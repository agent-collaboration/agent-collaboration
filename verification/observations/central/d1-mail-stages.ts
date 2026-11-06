/** Real public queue mail while the recipient Host is stopped, then held. No model turn. */
import { z } from "zod";
import type { D1Machine } from "./d1-harness.ts";
import type { Receipt } from "../../receipt.ts";
import { boardIdentity } from "../../clients/board-requests.ts";
import { field } from "../../exact-json.ts";
import { setTimeout } from "node:timers/promises";

export function observedD1Turns(machine: D1Machine): number {
  return machine.nativeTurnCount();
}

export async function requireNoD1Turns(
  machines: readonly D1Machine[],
  receipt: Receipt,
): Promise<void> {
  const observations = machines.map((machine) => ({
    service: machine.recipient.endpoint.serviceId,
    started: observedD1Turns(machine),
  }));
  await receipt.record("d1NoModelTurnObservation", { observations });
  if (observations.some((observation): boolean => observation.started !== 0))
    throw new Error("a native turn started during the no-model fixture drive");
}

export async function senderStage(input: {
  sender: D1Machine;
  item: string;
  kinds: readonly string[];
  receipt: Receipt;
  stage: string;
  timeoutMilliseconds?: number;
}): Promise<unknown> {
  const deadline = Date.now() + (input.timeoutMilliseconds ?? 30_000);
  let lastKind: string | undefined;
  let lastShown: unknown;
  while (Date.now() < deadline) {
    const shown = await input.sender.mcp.call("router_show", {
      caller: input.sender.recipient,
      reference: input.item,
    });
    const kind = z.string().parse(field(shown, "mail", "status", "kind"));
    lastShown = shown;
    if (lastKind !== kind) {
      await input.receipt.record("d1SenderStatusObserved", { stage: input.stage, shown });
      lastKind = kind;
    }
    if (input.kinds.includes(kind)) {
      await input.receipt.save(`queue-${input.stage}-sender.json`, shown);
      return shown;
    }
    await setTimeout(25);
  }
  await input.receipt.save(`queue-${input.stage}-unconfirmed.json`, { lastShown });
  throw new Error(`D1 sender ${input.stage} status not observed`);
}

export async function driveQueueHeld(input: {
  sender: D1Machine;
  recipient: D1Machine;
  receipt: Receipt;
}): Promise<void> {
  const { sender, recipient, receipt } = input;
  await recipient.stopForRestart();
  const item = boardIdentity();
  const request = {
    itemId: item,
    target: recipient.recipient,
    delivery: "queue",
    message: {
      kind: "agent",
      sender: sender.recipient,
      text: `D1 queued custody fixture ${item}. No model instruction is executed by this harness.`,
    },
  };
  const submitted = await sender.mcp.call("message_send", request);
  z.object({ queued: z.object({ item: z.literal(item) }) }).parse(submitted);
  await receipt.save("queue-submit.json", { request, submitted });
  // Current/intermediate public DTO says Sent; final D1 adapter must pin its actual public status name.
  await senderStage({ sender, item, kinds: ["sent"], receipt, stage: "accepted-offline" });
  await recipient.reopen();
  const loaded = z
    .object({ data: z.array(z.string()) })
    .parse(await recipient.native.rpc("thread/loaded/list", {}));
  await receipt.require(
    "queue recipient stays unloaded after Host restart",
    !loaded.data.includes(recipient.recipient.sessionId),
  );
  await senderStage({ sender, item, kinds: ["held"], receipt, stage: "held" });
  const held = await recipient.mcp.call("router_show", {
    caller: recipient.recipient,
    reference: item,
  });
  await receipt.save("queue-recipient-held.json", held);
  z.object({
    record: z.object({ pushId: z.literal(item), deliveryState: z.literal("held") }),
  }).parse(held);
  const repeated = await sender.mcp.call("message_send", request);
  await receipt.equal("same-id queue replay preserves admission", repeated, submitted);
  await senderStage({ sender, item, kinds: ["held"], receipt, stage: "held-replayed" });
  await receipt.record("d1QueueHeld", {
    item,
    sender: sender.recipient,
    recipient: recipient.recipient,
    modelTurn: "not started; debug Router required before readiness release",
    productScope: "intermediate real path; final D1 holder readbacks remain open",
  });
}

export async function unloadD1Recipient(recipient: D1Machine, receipt: Receipt): Promise<void> {
  const unsubscribed = await recipient.native.rpc("thread/unsubscribe", {
    threadId: recipient.recipient.sessionId,
  });
  z.object({ status: z.enum(["unsubscribed", "notLoaded", "notSubscribed"]) }).parse(unsubscribed);
  await receipt.record("d1RecipientUnsubscribed", { recipient: recipient.recipient, unsubscribed });
  const unloadedDeadline = Date.now() + 15_000;
  for (;;) {
    const loaded = z
      .object({ data: z.array(z.string()) })
      .parse(await recipient.native.rpc("thread/loaded/list", {}));
    if (!loaded.data.includes(recipient.recipient.sessionId)) {
      await receipt.record("d1RecipientUnloaded", { recipient: recipient.recipient, loaded });
      break;
    }
    if (Date.now() >= unloadedDeadline)
      throw new Error("native recipient did not reach unloaded state before queue send");
    await setTimeout(25);
  }
}

export async function driveLiveHeld(input: {
  sender: D1Machine;
  recipient: D1Machine;
  receipt: Receipt;
}): Promise<void> {
  const { sender, recipient, receipt } = input;
  await unloadD1Recipient(recipient, receipt);
  const item = boardIdentity();
  const request = {
    itemId: item,
    target: recipient.recipient,
    delivery: "queue",
    message: {
      kind: "agent",
      sender: sender.recipient,
      text: `D1 live held custody fixture ${item}.`,
    },
  };
  const submitted = await sender.mcp.call("message_send", request);
  z.object({ queued: z.object({ item: z.literal(item) }) }).parse(submitted);
  await receipt.save("queue-live-submit.json", { request, submitted });
  await senderStage({ sender, item, kinds: ["held"], receipt, stage: "live-held" });
  const shown = await recipient.mcp.call("router_show", {
    caller: recipient.recipient,
    reference: item,
  });
  z.object({
    record: z.object({ pushId: z.literal(item), deliveryState: z.literal("held") }),
  }).parse(shown);
  await receipt.save("queue-live-recipient.json", shown);
  const replayed = await sender.mcp.call("message_send", request);
  await receipt.equal("live same-id replay preserves admission", replayed, submitted);
  await receipt.record("d1LiveQueueHeld", {
    item,
    sender: sender.recipient,
    recipient: recipient.recipient,
    modelTurn: "not started; no readiness release",
    scope: "held stage characterization only; restart and terminal outcome open",
  });
}
