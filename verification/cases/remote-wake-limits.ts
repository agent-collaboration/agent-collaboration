import { z } from "zod";
import { boardIdentity } from "../clients/board-requests.ts";
import type { CaseRegistration } from "../coverage.ts";
import { field, textField } from "../exact-json.ts";
import { type Receipt } from "../receipt.ts";
import type { PreparedRun } from "../run-setup.ts";
import { observeWakeFire } from "./remote-wake-firing.ts";
import { wakeRefusal, wakeRequest, wakeSnapshotSchema } from "./remote-wake-inputs.ts";

export const remoteWakeLimitsCase = {
  id: "remote-wake-limits-and-real-five-minute-repeat",
  rows: ["OB4:14"],
  evidence: "journey",
} satisfies CaseRegistration;
async function rejectedLimits(context: PreparedRun, receipt: Receipt): Promise<void> {
  const [source, target] = context.machines;
  const matrix = [
    [{ kind: "after", seconds: 86_460n }, { kind: "none" }, { kind: "tooFarAhead", max: 86_400n }],
    [
      { kind: "after", seconds: 600n },
      { kind: "after", seconds: 87_001n },
      { kind: "lifetimeTooLong", max: 86_400n },
    ],
    [{ kind: "interval", seconds: 300n }, { kind: "none" }, { kind: "expiryRequired" }],
    [
      { kind: "interval", seconds: 300n },
      { kind: "after", seconds: 604_860n },
      { kind: "lifetimeTooLong", max: 604_800n },
    ],
    [
      { kind: "interval", seconds: 299n },
      { kind: "after", seconds: 7200n },
      { kind: "intervalTooShort", min: 300n },
    ],
    [
      { kind: "cron", expression: "* * * * *", timezone: "UTC" },
      { kind: "after", seconds: 7200n },
      { kind: "intervalTooShort", min: 300n },
    ],
  ] satisfies readonly (readonly [
    Readonly<Record<string, unknown>>,
    Readonly<Record<string, unknown>>,
    Readonly<Record<string, unknown>>,
  ])[];
  for (const [timing, expiry, expected] of matrix) {
    const request = wakeRequest({ source, target, timing, expiry });
    const refusal = await wakeRefusal(source.installation.mcp, request);
    const actual = z
      .object({ kind: z.string(), min: z.bigint().optional(), max: z.bigint().optional() })
      .parse(refusal);
    await receipt.equal("remote wake applies the contract's typed limit", actual, expected);
    await receipt.equal(
      "remote limit refusal creates no timer",
      field(refusal, "effects", "mutation"),
      "none",
    );
  }
}
export async function remoteWakeLimits(context: PreparedRun, receipt: Receipt): Promise<void> {
  await rejectedLimits(context, receipt);
  const [source, target] = context.machines;
  const known = new Set<string>();
  const failures: unknown[] = [];
  try {
    const due = new Date(Date.now() + 86_400_000).toISOString();
    const request = wakeRequest({
      source,
      target,
      timing: { kind: "at", at: due },
      expiry: { kind: "none" },
    });
    const boundary = wakeSnapshotSchema.parse(
      await source.installation.mcp.call("wake_send", request),
    );
    const boundaryId = boundary.definition.wakeupId;
    known.add(boundaryId);
    const owned = wakeSnapshotSchema.parse(
      await target.installation.mcp.call("wake_show", { wakeupId: boundaryId }),
    );
    await receipt.equal(
      "one-shot default lifetime ends 24 hours after its literal due time",
      Date.parse(z.string().parse(owned.definition.expiresAt)) - Date.parse(due),
      86_400_000,
    );
    await target.installation.mcp.call("wake_cancel", {
      operationId: boardIdentity(),
      wakeupId: boundaryId,
    });
    const repeating = wakeRequest({
      source,
      target,
      timing: { kind: "interval", seconds: 300n },
      expiry: { kind: "after", seconds: 604_800n },
    });
    const repeat = wakeSnapshotSchema.parse(
      await source.installation.mcp.call("wake_send", repeating),
    );
    const repeatId = repeat.definition.wakeupId;
    known.add(repeatId);
    await receipt.equal(
      "five-minute interval is accepted without shortening it",
      repeat.definition.timing,
      { kind: "interval", seconds: 300n },
    );
    const nativeInput = await observeWakeFire({
      target,
      wakeup: repeatId,
      receipt,
      seconds: 420,
      marker: `verification-wake-${textField(repeating, "operationId")}`,
    });
    const fired = wakeSnapshotSchema.parse(
      await target.installation.mcp.call("wake_show", { wakeupId: repeatId }),
    );
    await receipt.equal(
      "real first repeat is due five minutes after its owning anchor",
      Date.parse(z.string().parse(fired.firstFire?.dueAt)) - Date.parse(fired.definition.anchorAt),
      300_000,
    );
    await target.recipient.unique(nativeInput);
  } catch (error: unknown) {
    failures.push(error);
  } finally {
    for (const wakeupId of known) {
      try {
        await target.installation.mcp.call("wake_cancel", {
          operationId: boardIdentity(),
          wakeupId,
        });
      } catch (error: unknown) {
        failures.push(error);
      }
    }
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1)
    throw new AggregateError(failures, "remote wake limits or cleanup failed");
}
