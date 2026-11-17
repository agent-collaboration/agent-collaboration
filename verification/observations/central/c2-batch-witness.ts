// One wake may carry many requests; each needs its own current admission proof.
import assert from "node:assert/strict";
import { z } from "zod";
import {
  assertC2LiveReads,
  assertC2Restore,
  c2AttachmentSchema,
  type C2WitnessRecord,
} from "./c2-witness.ts";

export function assertC2BatchWitnesses(input: {
  readonly before: readonly C2WitnessRecord[];
  readonly records: readonly C2WitnessRecord[];
  readonly attachment: z.infer<typeof c2AttachmentSchema>;
  readonly correlationIds: readonly string[];
}): void {
  const correlations = z.array(z.uuidv7()).min(1).max(64).parse(input.correlationIds);
  assert.equal(
    new Set(correlations).size,
    correlations.length,
    "batch correlations must be unique",
  );
  assert.deepEqual(
    input.records.slice(0, input.before.length),
    input.before,
    "batch preserves original witness prefix",
  );
  for (const [index, record] of input.records.entries())
    assert.equal(
      record.sequence,
      index + 1,
      "external witness sequence must remain contiguous and ordered",
    );
  const first = correlations[0];
  assert.ok(first !== undefined);
  assertC2Restore({ ...input, correlationId: first }); // Exactly one constructor/handler for the whole frame.
  const fresh = input.records.slice(input.before.length);
  const constructor = fresh.find((record) => record.event.kind === "constructor");
  const handler = fresh.find((record) => record.event.kind === "handler");
  assert.ok(constructor !== undefined && handler !== undefined);
  assert.equal(
    fresh.filter((record) => record.event.kind === "restoreResult").length,
    correlations.length,
    "one restoration result per member, with no unrelated request",
  );
  assert.equal(
    fresh.filter((record) => record.event.kind === "alarmSnapshot").length,
    correlations.length,
    "one terminal alarm snapshot per member",
  );
  let priorTerminal = handler.sequence;
  for (const correlationId of correlations) {
    const restorations = fresh.filter(
      (record) =>
        record.event.kind === "restoreResult" && record.event.correlationId === correlationId,
    );
    const alarms = fresh.filter(
      (record) =>
        record.event.kind === "alarmSnapshot" && record.event.correlationId === correlationId,
    );
    assert.equal(restorations.length, 1, "exactly one matching restore per batch member");
    assert.equal(alarms.length, 1, "exactly one matching terminal snapshot per batch member");
    const restored = restorations[0];
    const alarm = alarms[0];
    assert.ok(restored?.event.kind === "restoreResult" && alarm?.event.kind === "alarmSnapshot");
    assert.equal(
      restored.event.outcome,
      "admitted",
      "semantic refusal still has its own live admission",
    );
    for (const record of [restored, alarm]) {
      assert.equal(
        record.instanceId,
        constructor.instanceId,
        "all members use the serving fresh instance",
      );
      assert.equal(
        record.event.objectId,
        constructor.event.objectId,
        "all members use the original object",
      );
      assert.equal(record.phase, "deciding", "member completes within the batch handler phase");
    }
    assert.ok(
      restored.sequence > priorTerminal,
      "frame order follows the previous member's terminal snapshot",
    );
    assertC2LiveReads({
      records: fresh,
      instanceId: constructor.instanceId,
      objectId: constructor.event.objectId,
      after: priorTerminal,
      before: restored.sequence,
    });
    assert.ok(
      alarm.sequence > restored.sequence,
      "terminal alarm follows this member's restoration",
    );
    assert.equal(alarm.event.earliestDueMs, null, "batch journey has no due work");
    assert.equal(alarm.event.alarmMs, null, "batch leaves actual platform alarm clear");
    priorTerminal = alarm.sequence;
  }
}
