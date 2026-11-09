/** Real public queue workloads; receiver launch, source qualification and store readback remain caller-owned. */
import { z } from "zod";
import type { D1Machine } from "../d1-harness-plan.ts";
import type { Receipt } from "../../../receipt.ts";
import { boardIdentity } from "../../../clients/board-requests.ts";
import { requireNoD1Turns, senderStage, unloadD1Recipient } from "../d1-mail-stages.ts";
import { ReceiverCustodyBarrier } from "./receiver-custody-barrier.ts";
import { requireWindowSnapshotAgreement, type D4Snapshot } from "./snapshot-observation.ts";
import {
  requireSubmittedHeldMessage,
  type SubmittedCustodyMessage,
} from "./custody-observation.ts";

export interface OfflineHeldReadbacks {
  readonly items: readonly string[];
  readonly recipientViews: readonly unknown[];
  readonly senderViews: readonly unknown[];
}
interface CustodyActors {
  readonly sender: D1Machine;
  readonly recipient: D1Machine;
  readonly receipt: Receipt;
}
export interface HeldWindowReadback {
  readonly snapshot: D4Snapshot;
  readonly wireCurrent: ReadonlyMap<string, bigint>;
  readonly recipientConnection: string;
}

async function queueItem(
  input: CustodyActors & { readonly phase: string; readonly status: "sent" | "held" },
): Promise<SubmittedCustodyMessage> {
  const item = boardIdentity();
  const request = {
    itemId: item,
    target: input.recipient.recipient,
    delivery: "queue",
    message: {
      kind: "agent",
      sender: input.sender.recipient,
      text: `D4 custody fixture ${input.phase}: ${item}`,
    },
  };
  await input.receipt.record("d4CustodyRequest", { phase: input.phase, request });
  const submitted = await input.sender.mcp.call("message_send", request);
  z.object({ queued: z.object({ item: z.literal(item) }) }).parse(submitted);
  await input.receipt.record("d4CustodyAdmission", { item, submitted });
  const stage = await senderStage({
    sender: input.sender,
    item,
    kinds: [input.status],
    receipt: input.receipt,
    stage: input.phase,
  });
  z.object({
    mail: z.object({ item: z.literal(item), status: z.object({ kind: z.literal(input.status) }) }),
  }).parse(stage);
  await requireNoD1Turns([input.sender, input.recipient], input.receipt);
  return { itemId: item, body: request.message.text };
}

async function heldItems(
  input: CustodyActors & {
    readonly submitted: readonly SubmittedCustodyMessage[];
  },
): Promise<OfflineHeldReadbacks> {
  const recipientViews: unknown[] = [],
    senderViews: unknown[] = [];
  for (const [index, original] of input.submitted.entries()) {
    const item = original.itemId;
    const senderView = await senderStage({
      sender: input.sender,
      item,
      kinds: ["held"],
      receipt: input.receipt,
      stage: `d4-held-${index}`,
    });
    z.object({
      mail: z.object({ item: z.literal(item), status: z.object({ kind: z.literal("held") }) }),
    }).parse(senderView);
    const recipientView = await input.recipient.mcp.call("router_show", {
      caller: input.recipient.recipient,
      reference: item,
    });
    requireSubmittedHeldMessage({ submitted: original, readback: recipientView });
    await input.receipt.save(`d4-held-${index}-recipient.json`, recipientView);
    senderViews.push(senderView);
    recipientViews.push(recipientView);
    await requireNoD1Turns([input.sender, input.recipient], input.receipt);
  }
  return { items: input.submitted.map((original) => original.itemId), recipientViews, senderViews };
}

/** B must reopen only after all ten Fleet admissions; this function never opens a recipient thread. */
export async function driveOfflineTen(input: CustodyActors): Promise<OfflineHeldReadbacks> {
  if (input.sender.recipient.endpoint.serviceId === input.recipient.recipient.endpoint.serviceId)
    throw new Error("offline custody requires distinct Router installations");
  await unloadD1Recipient(input.recipient, input.receipt);
  await requireNoD1Turns([input.sender, input.recipient], input.receipt);
  await input.recipient.stopForRestart();
  const submitted: SubmittedCustodyMessage[] = [];
  for (let index = 0; index < 10; index += 1)
    submitted.push(await queueItem({ ...input, phase: `d4-offline-${index}`, status: "sent" }));
  await input.receipt.save("d4-offline-items.json", {
    items: submitted.map((original) => original.itemId),
    submitted,
  });
  await input.recipient.reopen();
  return heldItems({ ...input, submitted });
}

async function collateralChat(
  input: CustodyActors & { readonly collateral: D1Machine; readonly phase: string },
): Promise<void> {
  const fromC = await queueItem({
    sender: input.collateral,
    recipient: input.sender,
    receipt: input.receipt,
    phase: `${input.phase}-c-to-a`,
    status: "held",
  });
  const fromA = await queueItem({
    sender: input.sender,
    recipient: input.collateral,
    receipt: input.receipt,
    phase: `${input.phase}-a-to-c`,
    status: "held",
  });
  await requireNoD1Turns([input.sender, input.recipient, input.collateral], input.receipt);
  await input.receipt.record("d4CollateralCustody", {
    phase: input.phase,
    fromC: fromC.itemId,
    fromA: fromA.itemId,
  });
}

async function heldWindow(input: {
  readonly read: () => Promise<HeldWindowReadback>;
  readonly receipt: Receipt;
  readonly phase: string;
  readonly expected?: number;
}): Promise<void> {
  const observed = await input.read();
  z.uuidv7().parse(observed.recipientConnection);
  requireWindowSnapshotAgreement(observed.snapshot, observed.wireCurrent);
  const expected = input.expected ?? 32;
  if (observed.wireCurrent.get(observed.recipientConnection) !== BigInt(expected))
    throw new Error(`the held recipient lacks its actual ${expected}-item window`);
  await input.receipt.save(`d4-window-${input.phase}.json`, {
    snapshot: observed.snapshot,
    wireCurrent: [...observed.wireCurrent],
    recipientConnection: observed.recipientConnection,
    runtimeQualified: false,
  });
}

async function drainWithCollateral(
  input: CustodyActors & {
    readonly collateral: D1Machine;
    readonly barrier: ReceiverCustodyBarrier;
    readonly readHeldWindow: () => Promise<HeldWindowReadback>;
    readonly total: number;
  },
): Promise<void> {
  let released = 0;
  while (released < input.total) {
    const group = Math.min(32, input.total - released);
    await input.barrier.waitForReached(released + group);
    const before = input.barrier.snapshot();
    if (before.phase !== "holding" || before.held < group)
      throw new Error("the next receiver drain group is not held");
    // These are actual current wire/S8 slots, never a count inferred from markers.
    await heldWindow({
      read: input.readHeldWindow,
      receipt: input.receipt,
      phase: `drain-${released}`,
      expected: group,
    });
    await collateralChat({ ...input, phase: `d4-held-drain-${released}` });
    await heldWindow({
      read: input.readHeldWindow,
      receipt: input.receipt,
      phase: `drain-${released}-after-collateral`,
      expected: group,
    });
    const count = input.barrier.releaseReached(group);
    if (count !== group) throw new Error("receiver drain group disappeared before release");
    released += count;
    await input.receipt.record("d4ReceiverDrainRelease", { before, released, group });
  }
  // Future retries must not encounter an abandoned apparatus cut.
  input.barrier.release();
}

/** The caller starts B with this barrier's existing runtime-test-support hook and supplies actual wire/S8 readback. */
export async function driveBacklogWithCollateral(
  input: CustodyActors & {
    readonly collateral: D1Machine;
    readonly barrier: ReceiverCustodyBarrier;
    readonly readHeldWindow: () => Promise<HeldWindowReadback>;
  },
): Promise<OfflineHeldReadbacks> {
  const machines = [input.sender, input.recipient, input.collateral];
  if (new Set(machines.map((machine): string => machine.recipient.endpoint.serviceId)).size !== 3)
    throw new Error("backlog custody requires three distinct Router installations");
  await requireNoD1Turns(machines, input.receipt);
  for (const machine of machines) await unloadD1Recipient(machine, input.receipt);
  await input.recipient.stopForRestart();
  const submitted: SubmittedCustodyMessage[] = [];
  try {
    for (let index = 0; index < 1000; index += 1) {
      submitted.push(await queueItem({ ...input, phase: `d4-offline-${index}`, status: "sent" }));
      if ((index + 1) % 100 === 0)
        await collateralChat({ ...input, phase: `d4-offline-chat-${index + 1}` });
    }
    await input.receipt.save("d4-offline-items.json", {
      items: submitted.map((original) => original.itemId),
      submitted: [...submitted],
    });
    await input.recipient.reopen();
    await input.barrier.waitForReached(1);
    await heldWindow({
      read: input.readHeldWindow,
      receipt: input.receipt,
      phase: "before-last-item",
    });
    submitted.push(await queueItem({ ...input, phase: "d4-after-welcome-1001", status: "sent" }));
    await input.receipt.save("d4-backlog-items.json", {
      items: submitted.map((original) => original.itemId),
      submitted,
    });
    await collateralChat({ ...input, phase: "d4-full-window-chat" });
    await heldWindow({
      read: input.readHeldWindow,
      receipt: input.receipt,
      phase: "after-collateral",
    });
    const held = input.barrier.snapshot();
    if (held.phase !== "holding" || held.held < 1)
      throw new Error("receiver commit cut was lost before release");
    await input.receipt.record("d4ReceiverControlledDrain", { held, items: submitted.length });
    await drainWithCollateral({ ...input, total: submitted.length });
    const result = await heldItems({ ...input, submitted });
    await requireNoD1Turns(machines, input.receipt);
    return result;
  } finally {
    if (input.barrier.snapshot().phase === "holding") {
      await input.receipt.record("d4ReceiverCutCleanup", { released: input.barrier.release() });
    }
  }
}
