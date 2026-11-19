// The Rust fixture drives actual auth, frames and SQLite. This observer checks its
// sanitized receipt; a parser pass alone is never a product execution result.
import assert from "node:assert/strict";
import { z } from "zod";
import { assertC2Restore, c2AttachmentSchema, c2RecordSchema } from "./c2-witness.ts";

const uuid = z.uuidv7();
const areaRef = z.object({ kind: z.literal("area"), id: uuid }).strict();
export const c2SessionSchema = z
  .object({
    machineId: uuid,
    sessionId: z
      .string()
      .min(1)
      .refine(
        (value): boolean => !value.includes("\0") && Buffer.byteLength(value, "utf8") <= 4096,
      ),
  })
  .strict();
const caller = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("session"), value: c2SessionSchema }).strict(),
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
    machineId: uuid,
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

export function correlateC2WorkspaceReply(
  correlationId: string,
  replyFrame: z.infer<typeof reply>,
): unknown {
  assert.equal(replyFrame.correlationId, correlationId, "Reply must match actual Request");
  return done.parse(replyFrame.result.reply.result).answer;
}

export function assertC2BaselineWorkspace(
  receipt: Pick<C2ProductReceipt, "baselineRequest" | "baselineReply">,
): string {
  const baselineOperation = z
    .object({ kind: z.literal("createWorkspace"), name: z.string().min(1) })
    .strict()
    .parse(receipt.baselineRequest.request.payload.request.operation);
  assert.equal(
    receipt.baselineRequest.request.payload.request.workspaceId,
    null,
    "baseline creates its own workspace",
  );
  assert.ok(
    receipt.baselineRequest.request.payload.request.requestId !== null,
    "baseline write has RequestId",
  );
  const baselineAnswer = z
    .object({
      kind: z.literal("workspaceCreated"),
      workspace: z
        .object({
          workspaceId: uuid,
          name: z.string(),
          state: z.literal("active"),
        })
        .strict(),
    })
    .strict()
    .parse(
      correlateC2WorkspaceReply(
        receipt.baselineRequest.request.correlationId,
        receipt.baselineReply,
      ),
    );
  assert.equal(
    baselineAnswer.workspace.name,
    baselineOperation.name,
    "baseline name comes from actual request",
  );
  return baselineAnswer.workspace.workspaceId;
}

function assertSemanticFrames(receipt: C2ProductReceipt): void {
  const baselineWorkspace = assertC2BaselineWorkspace(receipt);
  const created = z
    .object({
      kind: z.literal("created"),
      item: areaRef,
      position: z.number().int().safe().nonnegative(),
    })
    .strict()
    .parse(
      correlateC2WorkspaceReply(
        receipt.decidingRequest.request.correlationId,
        receipt.decidingReply,
      ),
    );
  const operation = z
    .object({
      kind: z.literal("createArea"),
      name: z.string().min(1),
      description: z.string().trim().min(1),
    })
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
  const workspace = receipt.decidingRequest.request.payload.request.workspaceId;
  assert.ok(workspace !== null, "CreateArea names a workspace");
  assert.equal(workspace, baselineWorkspace, "CreateArea uses the actual baseline workspace");
  assertC2AreaRead({
    request: receipt.readRequest,
    reply: receipt.readReply,
    workspace,
    areaId: created.item.id,
    name: operation.name,
  });
  const correlations = [receipt.baselineRequest, receipt.decidingRequest, receipt.readRequest].map(
    (frame) => frame.request.correlationId,
  );
  assert.equal(
    new Set(correlations).size,
    3,
    "distinct requests use distinct transport correlations",
  );
}

export function assertC2AreaRead(input: {
  readonly request: C2ProductReceipt["readRequest"];
  readonly reply: C2ProductReceipt["readReply"];
  readonly workspace: string;
  readonly areaId: string;
  readonly name: string;
}): void {
  const shown = z
    .object({ kind: z.literal("show"), item: areaRef })
    .strict()
    .parse(input.request.request.payload.request.operation);
  assert.equal(shown.item.id, input.areaId, "real read targets created area");
  assert.equal(
    input.request.request.payload.request.requestId,
    null,
    "read has no durable RequestId",
  );
  const readAnswer = z
    .object({
      kind: z.literal("item"),
      item: z.object({ kind: z.literal("area"), item: area }).strict(),
    })
    .strict()
    .parse(correlateC2WorkspaceReply(input.request.request.correlationId, input.reply));
  assert.equal(readAnswer.item.item.areaId, input.areaId);
  assert.equal(readAnswer.item.item.name, input.name);
  assert.equal(readAnswer.item.item.workspaceId, input.workspace);
  assert.equal(input.request.request.payload.request.workspaceId, input.workspace);
}

function assertSessionCaller(receipt: C2ProductReceipt): void {
  const originalCaller = receipt.baselineRequest.request.caller;
  assert.equal(originalCaller.kind, "session", "positive journey uses an ordinary session caller");
  for (const frame of [receipt.baselineRequest, receipt.decidingRequest, receipt.readRequest]) {
    assert.equal(
      frame.request.caller.kind,
      "session",
      "positive journey uses an ordinary session caller",
    );
    assert.deepEqual(
      frame.request.caller,
      originalCaller,
      "same stamped session drives all semantic frames",
    );
    assert.equal(
      frame.request.caller.value.machineId,
      receipt.machineId,
      "caller uses the Router machine ID",
    );
  }
}

export function originalC2Attachment(
  receipt: Pick<
    C2ProductReceipt,
    "beforeEvents" | "organization" | "machineId" | "heartbeatEvents"
  >,
): z.infer<typeof c2AttachmentSchema> {
  const upgrades = receipt.beforeEvents.filter((record) => record.event.kind === "upgrade");
  assert.equal(upgrades.length, 1, "one original socket upgrade");
  const upgrade = upgrades[0];
  assert.ok(upgrade?.event.kind === "upgrade");
  assert.equal(upgrade.event.attachments.length, 1, "one original attachment");
  const attachment = upgrade.event.attachments[0];
  assert.ok(attachment !== undefined);
  assert.equal(attachment.organizationId, receipt.organization);
  assert.equal(attachment.machineId, receipt.machineId, "attachment uses the Router machine ID");
  assert.ok(
    receipt.beforeEvents.some(
      (record) =>
        record.event.kind === "constructor" &&
        record.instanceId === upgrade.instanceId &&
        record.sequence < upgrade.sequence &&
        record.event.objectId === upgrade.event.objectId,
    ),
    "initial upgrade has its actual serving constructor",
  );
  for (const snapshot of receipt.heartbeatEvents)
    assert.deepEqual(snapshot, receipt.beforeEvents, "each pong adds no observed witnesses");
  return attachment;
}

function assertSocketWitnesses(receipt: C2ProductReceipt): void {
  const attachment = originalC2Attachment(receipt);
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
  for (const correlationId of [
    receipt.baselineRequest.request.correlationId,
    receipt.decidingRequest.request.correlationId,
  ]) {
    const snapshots = receipt.decidingEvents.filter(
      (record) =>
        record.event.kind === "alarmSnapshot" && record.event.correlationId === correlationId,
    );
    assert.equal(snapshots.length, 1, "one actual alarm snapshot per semantic request");
    const snapshot = snapshots[0];
    assert.ok(snapshot?.event.kind === "alarmSnapshot");
    assert.equal(snapshot.event.earliestDueMs, null, "no due work in this declared journey");
    assert.equal(snapshot.event.alarmMs, null, "semantic journey leaves actual alarm clear");
  }
}

export function assertC2ProductReceipt(raw: unknown): C2ProductReceipt {
  const receipt = c2ProductReceiptSchema.parse(raw);
  assert.equal(receipt.welcome.machineId, receipt.machineId, "Welcome uses the Router machine ID");
  assertSemanticFrames(receipt);
  assertSessionCaller(receipt);
  assertSocketWitnesses(receipt);
  const idIndex = receipt.store.columns.indexOf("area_id");
  const nameIndex = receipt.store.columns.indexOf("name");
  assert.ok(idIndex >= 0 && nameIndex >= 0, "owning SQLite read returns area_id and name");
  assert.equal(receipt.store.rows.length, 1, "exactly one owning SQLite area row");
  assert.deepEqual(receipt.store.rows[0]?.[idIndex], { kind: "text", value: receipt.area.areaId });
  assert.deepEqual(receipt.store.rows[0]?.[nameIndex], { kind: "text", value: receipt.area.name });
  return receipt;
}
