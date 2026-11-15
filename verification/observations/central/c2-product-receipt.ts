// The Rust fixture drives actual auth, frames and SQLite. This observer checks its
// sanitized receipt; a parser pass alone is never a product execution result.
import assert from "node:assert/strict";
import { z } from "zod";
import { assertC2Restore, c2RecordSchema } from "./c2-witness.ts";

const uuid = z.uuidv7();
const areaRef = z.object({ kind: z.literal("area"), id: uuid }).strict();
const session = z
  .object({ machineId: uuid, endpoint: z.string().min(1), sessionId: uuid })
  .strict();
const caller = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("session"), value: session }).strict(),
  z
    .object({
      kind: z.literal("person"),
      value: z.object({ machineId: uuid, personId: uuid }).strict(),
    })
    .strict(),
]);
const request = z
  .object({
    kind: z.literal("request"),
    request: z
      .object({
        correlationId: uuid,
        caller,
        payload: z
          .object({
            kind: z.literal("workspace"),
            request: z
              .object({
                requestId: uuid.nullable(),
                workspaceId: uuid.nullable(),
                operation: z.json(),
              })
              .strict(),
          })
          .strict(),
      })
      .strict(),
  })
  .strict();
const reply = z
  .object({
    kind: z.literal("reply"),
    correlationId: uuid,
    result: z
      .object({
        kind: z.literal("workspace"),
        reply: z.object({ result: z.json(), actors: z.array(z.json()) }).strict(),
      })
      .strict(),
  })
  .strict();
const done = z.object({ kind: z.literal("done"), answer: z.json() }).strict();
const area = z
  .object({ areaId: uuid, workspaceId: uuid, name: z.string().min(1), state: z.literal("open") })
  .strict();
const sqlValue = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("null") }).strict(),
  z.object({ kind: z.literal("text"), value: z.string() }).strict(),
  z.object({ kind: z.literal("integer"), value: z.number().int().safe() }).strict(),
  z.object({ kind: z.literal("blob"), value: z.array(z.number().int().min(0).max(255)) }).strict(),
]);

export const c2ProductReceiptSchema = z
  .object({
    kind: z.literal("c2Hibernation"),
    organization: uuid,
    installationId: uuid,
    welcome: z
      .object({
        kind: z.literal("welcome"),
        machineId: uuid,
        centralTime: z.iso.datetime({ offset: true }),
      })
      .strict(),
    baselineRequest: request,
    baselineReply: reply,
    decidingRequest: request,
    decidingReply: reply,
    readRequest: request,
    readReply: reply,
    beforeEvents: z.array(c2RecordSchema),
    heartbeatEvents: z.array(z.array(c2RecordSchema)).length(3),
    decidingEvents: z.array(c2RecordSchema),
    area: area.pick({ areaId: true, name: true }),
    store: z.object({ columns: z.array(z.string()), rows: z.array(z.array(sqlValue)) }).strict(),
    disposed: z.literal(true),
  })
  .strict();
export type C2ProductReceipt = z.infer<typeof c2ProductReceiptSchema>;

function correlate(
  requestFrame: z.infer<typeof request>,
  replyFrame: z.infer<typeof reply>,
): unknown {
  assert.equal(
    replyFrame.correlationId,
    requestFrame.request.correlationId,
    "Reply must match actual Request",
  );
  return done.parse(replyFrame.result.reply.result).answer;
}

export function assertC2ProductReceipt(raw: unknown): C2ProductReceipt {
  const receipt = c2ProductReceiptSchema.parse(raw);
  assert.equal(
    receipt.welcome.machineId,
    receipt.installationId,
    "Welcome uses public installation",
  );
  correlate(receipt.baselineRequest, receipt.baselineReply);
  const created = z
    .object({
      kind: z.literal("created"),
      item: areaRef,
      position: z.number().int().safe().nonnegative(),
    })
    .strict()
    .parse(correlate(receipt.decidingRequest, receipt.decidingReply));
  const operation = z
    .object({ kind: z.literal("createArea"), name: z.string().min(1) })
    .strict()
    .parse(receipt.decidingRequest.request.payload.request.operation);
  assert.equal(
    receipt.area.areaId,
    created.item.id,
    "receipt ID must come from actual CreateArea Reply",
  );
  assert.equal(receipt.area.name, operation.name, "stored name comes from the actual Request");
  assert.ok(
    receipt.decidingRequest.request.payload.request.requestId !== null,
    "write has durable RequestId",
  );
  const shown = z
    .object({ kind: z.literal("show"), item: areaRef })
    .strict()
    .parse(receipt.readRequest.request.payload.request.operation);
  assert.equal(shown.item.id, created.item.id, "real read targets created area");
  assert.equal(
    receipt.readRequest.request.payload.request.requestId,
    null,
    "read has no durable RequestId",
  );
  const readAnswer = z
    .object({
      kind: z.literal("item"),
      item: z.object({ kind: z.literal("area"), item: area }).strict(),
    })
    .strict()
    .parse(correlate(receipt.readRequest, receipt.readReply));
  assert.equal(readAnswer.item.item.areaId, created.item.id);
  assert.equal(readAnswer.item.item.name, operation.name);
  const workspace = receipt.decidingRequest.request.payload.request.workspaceId;
  assert.ok(workspace !== null, "CreateArea names a workspace");
  assert.equal(readAnswer.item.item.workspaceId, workspace);
  assert.equal(receipt.readRequest.request.payload.request.workspaceId, workspace);
  const correlations = [receipt.baselineRequest, receipt.decidingRequest, receipt.readRequest].map(
    (frame) => frame.request.correlationId,
  );
  assert.equal(
    new Set(correlations).size,
    3,
    "distinct requests use distinct transport correlations",
  );
  for (const frame of [receipt.baselineRequest, receipt.decidingRequest, receipt.readRequest])
    assert.equal(
      frame.request.caller.value.machineId,
      receipt.installationId,
      "caller uses installation",
    );
  const upgrades = receipt.beforeEvents.filter((record) => record.event.kind === "upgrade");
  assert.equal(upgrades.length, 1, "one original socket upgrade");
  const upgrade = upgrades[0];
  assert.ok(upgrade?.event.kind === "upgrade");
  assert.equal(upgrade.event.attachments.length, 1, "one original attachment");
  const attachment = upgrade.event.attachments[0];
  assert.ok(attachment !== undefined);
  assert.equal(attachment.organizationId, receipt.organization);
  assert.equal(attachment.installationId, receipt.installationId);
  for (const snapshot of receipt.heartbeatEvents)
    assert.deepEqual(snapshot, receipt.beforeEvents, "each pong adds no observed witnesses");
  assert.deepEqual(
    receipt.decidingEvents.slice(0, receipt.beforeEvents.length),
    receipt.beforeEvents,
    "deciding snapshot preserves the full original witness prefix",
  );
  assertC2Restore({
    records: receipt.decidingEvents,
    before: receipt.beforeEvents,
    attachment,
    correlationId: receipt.decidingRequest.request.correlationId,
  });
  const idIndex = receipt.store.columns.indexOf("area_id");
  const nameIndex = receipt.store.columns.indexOf("name");
  assert.ok(idIndex >= 0 && nameIndex >= 0, "owning SQLite read returns area_id and name");
  assert.equal(receipt.store.rows.length, 1, "exactly one owning SQLite area row");
  assert.deepEqual(receipt.store.rows[0]?.[idIndex], { kind: "text", value: created.item.id });
  assert.deepEqual(receipt.store.rows[0]?.[nameIndex], { kind: "text", value: operation.name });
  return receipt;
}
