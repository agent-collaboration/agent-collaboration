// The approved mixed batch is Workspace/Mail/Workspace, with independent Replies.
import assert from "node:assert/strict";
import { z } from "zod";
import { assertC2BatchWitnesses } from "./c2-batch-witness.ts";
import {
  assertC2AreaRead,
  assertC2BaselineWorkspace,
  c2ProductReceiptSchema,
  c2SessionSchema,
  correlateC2WorkspaceReply,
  originalC2Attachment,
} from "./c2-product-receipt.ts";

const single = c2ProductReceiptSchema.shape;
const workspaceMember = single.decidingRequest.shape.request;
const mailMember = workspaceMember.omit({ payload: true }).extend({
  payload: z
    .object({
      kind: z.literal("mail"),
      request: z
        .object({
          requestId: z.uuidv7(),
          operation: z
            .object({
              kind: z.literal("setWake"),
              target: c2SessionSchema,
              dueAt: z.iso.datetime({ offset: true }),
              text: z.string().min(1),
            })
            .strict(),
        })
        .strict(),
    })
    .strict(),
});
const mailReply = single.decidingReply
  .omit({ result: true })
  .extend({ result: z.object({ kind: z.literal("mail"), reply: z.json() }).strict() });
const batchFrame = z
  .object({
    kind: z.literal("batch"),
    requests: z
      .array(z.union([workspaceMember, mailMember]))
      .min(1)
      .max(64),
  })
  .strict();
export const c2BatchReceiptSchema = c2ProductReceiptSchema
  .omit({
    kind: true,
    decidingRequest: true,
    decidingReply: true,
    readRequest: true,
    readReply: true,
    area: true,
  })
  .extend({
    kind: z.literal("c2BatchHibernation"),
    batchRequest: batchFrame,
    batchReplies: z.array(z.union([single.decidingReply, mailReply])).length(3),
    readRequests: z.array(single.readRequest).length(2),
    readReplies: z.array(single.readReply).length(2),
    areas: z.array(single.area).length(2),
    wakes: single.store,
  });
export type C2BatchReceipt = z.infer<typeof c2BatchReceiptSchema>;

function assertBatchFrames(receipt: C2BatchReceipt): void {
  assert.equal(receipt.batchRequest.requests.length, 3, "declared journey has three batch members");
  const workspace = assertC2BaselineWorkspace(receipt);
  const originalCaller = receipt.baselineRequest.request.caller;
  assert.ok(originalCaller.kind === "session", "batch journey uses an ordinary session caller");
  assert.equal(
    originalCaller.value.machineId,
    receipt.installationId,
    "batch caller uses installation",
  );
  assert.equal(receipt.welcome.machineId, receipt.installationId, "Welcome uses installation");
  const correlations: string[] = [receipt.baselineRequest.request.correlationId];
  const writeIds: string[] = [];
  for (const [index, member] of receipt.batchRequest.requests.entries()) {
    assert.deepEqual(
      member.caller,
      originalCaller,
      "every batch member uses the original stamped session",
    );
    const requestId = member.payload.request.requestId;
    assert.ok(requestId !== null, "batch members are writes with durable IDs");
    writeIds.push(requestId);
    correlations.push(member.correlationId);
    assert.equal(
      receipt.batchReplies[index]?.correlationId,
      member.correlationId,
      "Reply follows member frame order and correlation",
    );
  }
  assert.equal(new Set(writeIds).size, 3, "three independent write IDs");
  for (const [readIndex, memberIndex] of [0, 2].entries()) {
    const member = workspaceMember.parse(receipt.batchRequest.requests[memberIndex]);
    const reply = single.decidingReply.parse(receipt.batchReplies[memberIndex]);
    const operation = z
      .object({
        kind: z.literal("createArea"),
        name: z.string().min(1),
        description: z.string().refine((text) => text.trim().length > 0),
      })
      .strict()
      .parse(member.payload.request.operation);
    assert.equal(
      member.payload.request.workspaceId,
      workspace,
      "batch writes use baseline workspace",
    );
    const created = z
      .object({
        kind: z.literal("created"),
        item: z.object({ kind: z.literal("area"), id: z.uuidv7() }).strict(),
        position: z.number().int().safe().nonnegative(),
      })
      .strict()
      .parse(correlateC2WorkspaceReply(member.correlationId, reply));
    const area = receipt.areas[readIndex];
    const readRequest = receipt.readRequests[readIndex];
    const readReply = receipt.readReplies[readIndex];
    assert.ok(area !== undefined && readRequest !== undefined && readReply !== undefined);
    assert.deepEqual(
      area,
      { areaId: created.item.id, name: operation.name },
      "area receipt binds actual member input and result",
    );
    assert.deepEqual(
      readRequest.request.caller,
      originalCaller,
      "read uses original stamped session",
    );
    assertC2AreaRead({
      request: readRequest,
      reply: readReply,
      workspace,
      areaId: area.areaId,
      name: area.name,
      description: operation.description,
    });
    correlations.push(readRequest.request.correlationId);
  }
  assert.equal(
    new Set(correlations).size,
    6,
    "baseline, three members and two reads use distinct correlations",
  );
  assert.equal(
    new Set(receipt.areas.map((area) => area.areaId)).size,
    2,
    "later success creates a distinct area",
  );
  assert.equal(
    new Set(receipt.areas.map((area) => area.name)).size,
    2,
    "successful requests use distinct literal names",
  );
  assertMiddleWake(receipt, originalCaller.value);
}

function assertMiddleWake(
  receipt: C2BatchReceipt,
  originalSession: z.infer<typeof c2SessionSchema>,
): void {
  const refused = mailMember.parse(receipt.batchRequest.requests[1]);
  const refusal = mailReply.parse(receipt.batchReplies[1]);
  assert.deepEqual(
    refused.payload.request.operation.target,
    originalSession,
    "past wake targets the same session",
  );
  assert.equal(
    refused.payload.request.operation.dueAt,
    "2030-03-17T17:45:00Z",
    "declared past grid timestamp",
  );
  assert.ok(
    Date.parse(refused.payload.request.operation.dueAt) < Date.parse(receipt.welcome.centralTime),
    "wake is past relative to injected central time",
  );
  assert.deepEqual(
    refusal.result.reply,
    {
      kind: "refused",
      value: { kind: "timingViolation", value: { field: "dueAt", requirement: "futureTime" } },
    },
    "middle refusal is the selected FutureTime rule",
  );
}

export function assertC2BatchReceipt(raw: unknown): C2BatchReceipt {
  const receipt = c2BatchReceiptSchema.parse(raw);
  assertBatchFrames(receipt);
  assertC2BatchWitnesses({
    before: receipt.beforeEvents,
    records: receipt.decidingEvents,
    attachment: originalC2Attachment(receipt),
    correlationIds: receipt.batchRequest.requests.map((member) => member.correlationId),
  });
  assert.deepEqual(receipt.store.columns, ["area_id", "name"], "read actual area columns");
  const expected = [...receipt.areas]
    .sort((left, right) => (left.name < right.name ? -1 : left.name > right.name ? 1 : 0))
    .map((area) => [
      { kind: "text", value: area.areaId },
      { kind: "text", value: area.name },
    ]);
  assert.deepEqual(
    receipt.store.rows,
    expected,
    "exact two owning area rows after the mixed batch",
  );
  assert.deepEqual(
    receipt.wakes,
    { columns: ["wake_count"], rows: [[{ kind: "integer", value: 0 }]] },
    "refused wake stores zero owning central rows",
  );
  return receipt;
}
