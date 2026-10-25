import { z } from "zod";
import { boardIdentity } from "../clients/board-requests.ts";
import type { CaseRegistration } from "../coverage.ts";
import { field, textField } from "../exact-json.ts";
import { type Receipt } from "../receipt.ts";
import type { PreparedRun } from "../run-setup.ts";
import { observeWakeFire } from "./remote-wake-firing.ts";
import {
  cancelKnownWakes,
  wakeRefusal,
  wakeRequest,
  wakeSnapshotSchema,
} from "./remote-wake-inputs.ts";

export const remoteWakesCase = {
  id: "remote-wake-ownership-replace-and-fire",
  rows: ["OB4:14"],
  evidence: "journey",
} satisfies CaseRegistration;
async function observeOwnedWake(input: {
  context: PreparedRun;
  receipt: Receipt;
  request: Readonly<Record<string, unknown>>;
  known: Set<string>;
}): Promise<string> {
  const { request, receipt, known } = input;
  const [source, target] = input.context.machines;
  const original = await source.installation.mcp.call("wake_send", request);
  const first = wakeSnapshotSchema.parse(original);
  const firstId = first.definition.wakeupId;
  known.add(firstId);
  const owned = wakeSnapshotSchema.parse(
    await target.installation.mcp.call("wake_show", { wakeupId: firstId }),
  );
  await receipt.equal("remote timer is active on B", owned.state, "active");
  await receipt.equal(
    "remote timer preserves literal target and content",
    owned.definition.message,
    field(request, "message"),
  );
  await receipt.equal(
    "remote timer preserves its declared creator",
    owned.definition.createdBy,
    request.requestedBy,
  );
  await receipt.equal(
    "remote create replay preserves the original snapshot",
    await source.installation.mcp.call("wake_send", request),
    original,
  );
  const refused = await wakeRefusal(source.installation.mcp, {
    ...request,
    operationId: boardIdentity(),
  });
  await receipt.equal(
    "second remote wake names the occupying wake and creator",
    z
      .object({ kind: z.literal("wakeExists"), wakeup: z.string(), creator: z.unknown() })
      .parse(refused),
    { kind: "wakeExists", wakeup: firstId, creator: request.requestedBy },
  );
  await source.installation.mcp.call("wake_pause", {
    operationId: boardIdentity(),
    wakeupId: firstId,
  });
  await receipt.equal(
    "remote pause affects B's timer",
    field(await target.installation.mcp.call("wake_show", { wakeupId: firstId }), "state"),
    "paused",
  );
  const pausedRefusal = await wakeRefusal(source.installation.mcp, {
    ...request,
    operationId: boardIdentity(),
  });
  await receipt.equal(
    "paused remote timer still occupies its slot",
    field(pausedRefusal, "kind"),
    "wakeExists",
  );
  return firstId;
}
async function replaceAndFire(input: {
  context: PreparedRun;
  receipt: Receipt;
  request: Readonly<Record<string, unknown>>;
  known: Set<string>;
  firstId: string;
}): Promise<void> {
  const { request, receipt, known, firstId } = input;
  const [source, target] = input.context.machines;
  const replacementRequest = {
    ...request,
    operationId: boardIdentity(),
    replace: true,
    timing: { kind: "after", seconds: 2n },
    expiry: { kind: "after", seconds: 60n },
  };
  const replacementRaw = await source.installation.mcp.call("wake_send", replacementRequest);
  const replacement = wakeSnapshotSchema.parse(replacementRaw);
  const replacementId = replacement.definition.wakeupId;
  known.add(replacementId);
  await receipt.require("replacement has a fresh timer identity", replacementId !== firstId);
  await receipt.equal(
    "replacement cancels the old timer on B",
    field(await target.installation.mcp.call("wake_show", { wakeupId: firstId }), "state"),
    "cancelled",
  );
  await receipt.equal(
    "replacement replay does not replace twice",
    await source.installation.mcp.call("wake_send", replacementRequest),
    replacementRaw,
  );
  const nativeInput = await observeWakeFire({
    target,
    wakeup: replacementId,
    marker: `verification-wake-${textField(request, "operationId")}`,
    receipt,
    seconds: 120,
  });
  await receipt.equal(
    "one-shot remote timer finishes on B",
    field(await target.installation.mcp.call("wake_show", { wakeupId: replacementId }), "state"),
    "finished",
  );
  await target.recipient.unique(nativeInput);
}
export async function remoteWakes(context: PreparedRun, receipt: Receipt): Promise<void> {
  const [source, target] = context.machines;
  const request = wakeRequest({
    source,
    target,
    timing: { kind: "after", seconds: 600n },
    expiry: { kind: "after", seconds: 660n },
  });
  const known = new Set<string>();
  const failures: unknown[] = [];
  try {
    const firstId = await observeOwnedWake({ context, receipt, request, known });
    await replaceAndFire({ context, receipt, request, known, firstId });
  } catch (error: unknown) {
    failures.push(error);
  } finally {
    failures.push(...(await cancelKnownWakes(target.installation.mcp, known)));
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1)
    throw new AggregateError(failures, "remote wake assertion or cleanup failed");
}
