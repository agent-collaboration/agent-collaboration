import { boardIdentity } from "../clients/board-requests.ts";
import type { CaseRegistration } from "../coverage.ts";
import { field } from "../exact-json.ts";
import { ObservationOpen, type Receipt } from "../receipt.ts";
import type { PreparedRun } from "../run-setup.ts";
import type { JourneyMachine } from "../run-context.ts";
import {
  cancelKnownWakes,
  wakeRecords,
  wakeRefusal,
  wakeRequest,
  wakeSnapshotSchema,
} from "./remote-wake-inputs.ts";

export const remoteWakeExpiryCase = {
  id: "remote-wake-expiry-before-recipient-admission",
  rows: ["OB4:14"],
  evidence: "journey",
} satisfies CaseRegistration;
async function cleanupExpiredWakes(input: {
  target: JourneyMachine;
  healthyId: string;
  request: Readonly<Record<string, unknown>> | undefined;
}): Promise<readonly unknown[]> {
  const failures: unknown[] = [];
  const known = new Set([input.healthyId]);
  try {
    for (const record of await wakeRecords(input.target.installation.mcp)) {
      const wake = wakeSnapshotSchema.parse(record);
      if (
        input.request !== undefined &&
        field(wake.definition.message.content, "text") ===
          field(input.request, "message", "content", "text")
      )
        known.add(wake.definition.wakeupId);
    }
  } catch (error: unknown) {
    failures.push(error);
  }
  failures.push(...(await cancelKnownWakes(input.target.installation.mcp, known)));
  return failures;
}
export async function remoteWakeExpiry(context: PreparedRun, receipt: Receipt): Promise<void> {
  const [source, target] = context.machines;
  const gate = context.gates.get(target.installation.machine.name);
  if (gate === undefined) throw new ObservationOpen("recipient Session gate is unavailable");
  const connected = await gate.ready();
  const healthyRequest = wakeRequest({
    source,
    target,
    timing: { kind: "after", seconds: 600n },
    expiry: { kind: "after", seconds: 660n },
  });
  const healthy = wakeSnapshotSchema.parse(
    await source.installation.mcp.call("wake_send", healthyRequest),
  );
  const healthyId = healthy.definition.wakeupId;
  const failures: unknown[] = [];
  let expiredRequest: Readonly<Record<string, unknown>> | undefined;
  try {
    await receipt.equal(
      "healthy control exists on the actual recipient",
      field(
        await target.installation.mcp.call("wake_show", { wakeupId: healthyId }),
        "definition",
        "wakeupId",
      ),
      healthyId,
    );
    await target.installation.mcp.call("wake_cancel", {
      operationId: boardIdentity(),
      wakeupId: healthyId,
    });
    const baseline = (await wakeRecords(target.installation.mcp))
      .map((record): string => wakeSnapshotSchema.parse(record).definition.wakeupId)
      .sort();
    await receipt.require(
      "wake inventory contains the real healthy control",
      baseline.includes(healthyId),
    );
    gate.pause();
    const now = Date.now();
    const expiry = new Date(now + 6000).toISOString();
    expiredRequest = wakeRequest({
      source,
      target,
      timing: { kind: "at", at: new Date(now + 2000).toISOString() },
      expiry: { kind: "at", at: expiry },
    });
    const expired = await wakeRefusal(source.installation.mcp, expiredRequest);
    await receipt.equal(
      "remote mail expires before B admits the timer",
      field(expired, "kind"),
      "deadlineExpired",
    );
    await receipt.require(
      "the frozen validUntil elapsed before replay",
      Date.now() >= Date.parse(expiry),
    );
    await receipt.equal(
      "same operation after validUntil retains its typed deadline answer",
      field(await wakeRefusal(source.installation.mcp, expiredRequest), "kind"),
      "deadlineExpired",
    );
    gate.resume();
    await gate.ready(connected.welcomes);
    await receipt.equal(
      "reconnected B creates no expired timer",
      (await wakeRecords(target.installation.mcp))
        .map((record): string => wakeSnapshotSchema.parse(record).definition.wakeupId)
        .sort(),
      baseline,
    );
    await receipt.equal(
      "late replay never extends the frozen deadline",
      field(await wakeRefusal(source.installation.mcp, expiredRequest), "kind"),
      "deadlineExpired",
    );
    await receipt.equal(
      "late replay adds no timer after reconnection",
      (await wakeRecords(target.installation.mcp))
        .map((record): string => wakeSnapshotSchema.parse(record).definition.wakeupId)
        .sort(),
      baseline,
    );
  } catch (error: unknown) {
    failures.push(error);
  } finally {
    gate.resume();
    try {
      await gate.ready();
    } catch (error: unknown) {
      failures.push(error);
    }
    failures.push(...(await cleanupExpiredWakes({ target, healthyId, request: expiredRequest })));
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1)
    throw new AggregateError(failures, "expired wake assertion or cleanup failed");
}
