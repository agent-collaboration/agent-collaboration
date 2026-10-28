import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { isDeepStrictEqual } from "node:util";
import { CopyHold } from "../clients/copy-hold.ts";
import {
  feedCursorSchema,
  feedAtLeast,
  projectPlacementSchema,
} from "../clients/board-requests.ts";
import type { PreparedRun } from "../run-setup.ts";
import { projectForCases } from "../run-setup.ts";
import type { CaseRegistration } from "../coverage.ts";
import { ObservationOpen, type Receipt } from "../receipt.ts";
import { freezeNotification, finishNotification } from "./notifications.ts";
import { sendAndRepeatRemoteMessage } from "./direct-messages.ts";

export const heldCopyCase = {
  id: "held-copy-acknowledgement",
  rows: ["OB4:4"],
  evidence: "journey",
} satisfies CaseRegistration;
const viewSchema = z.object({ asOf: feedCursorSchema, replicatedThrough: feedCursorSchema });
export async function heldCopy(context: PreparedRun, receipt: Receipt): Promise<void> {
  const [source, target] = context.machines;
  const notification = {
    source: source.installation,
    actor: source.actor,
    project: await projectForCases(source.installation, source.actor),
    recipients: [target],
    receipt,
  };
  const hold = new CopyHold({
    owner: context.central.owner,
    service: (await source.installation.identity()).serviceId,
    project: notification.project,
    receipt,
  });
  const before = target.recipient.position();
  let frozen;
  let causePosition;
  try {
    await hold.arm();
    frozen = await freezeNotification(notification);
    await hold.cause(frozen.post);
    const post = z
      .object({ postId: z.literal(frozen.post), position: projectPlacementSchema })
      .parse(await source.installation.mcp.call("post_show", { postId: frozen.post }));
    causePosition = post.position.position.cursor;
    const view = viewSchema.parse(
      await source.installation.mcp.call("view_work", {
        projectId: notification.project,
        includeRemoved: false,
      }),
    );
    await receipt.require(
      "held cause is outside the acknowledged prefix",
      feedAtLeast(view.asOf, frozen.through) && !feedAtLeast(view.replicatedThrough, causePosition),
    );
    await sendAndRepeatRemoteMessage({
      sender: source.installation,
      senderSession: source.recipient.target,
      recipient: target.recipient,
      receipt,
    });
    await target.recipient.absent(z.string().parse(frozen.items[0]), 3000, before);
  } finally {
    await hold.close();
  }
  if (frozen === undefined || causePosition === undefined)
    throw new ObservationOpen("cause hold was not observed");
  const deadline = Date.now() + 30_000;
  let acknowledged = false;
  do {
    const view = viewSchema.parse(
      await source.installation.mcp.call("view_work", {
        projectId: notification.project,
        includeRemoved: false,
      }),
    );
    acknowledged =
      isDeepStrictEqual(view.asOf, view.replicatedThrough) &&
      feedAtLeast(view.asOf, frozen.through) &&
      feedAtLeast(view.asOf, causePosition);
    if (acknowledged) break;
    await setTimeout(100);
  } while (Date.now() < deadline);
  await receipt.require(
    "released cause and frozen handoff reach the acknowledged writer prefix",
    acknowledged,
  );
  await finishNotification(notification, frozen);
}
