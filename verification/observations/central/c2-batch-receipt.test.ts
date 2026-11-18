import assert from "node:assert/strict";
import { test } from "node:test";
import {
  assertC2BatchReceipt,
  c2BatchReceiptSchema,
  type C2BatchReceipt,
} from "./c2-batch-receipt.ts";
import { c2ProductReceiptSchema, type C2ProductReceipt } from "./c2-product-receipt.ts";
import { c2RecordSchema, type C2WitnessRecord } from "./c2-witness.ts";
import { c2BatchSelector, c2ProductSelector, requireSingleC2Selection } from "./c2-product-run.ts";

const organization = "019f0000-0000-7000-8000-000000000001";
const installation = "019f0000-0000-7000-8000-000000000002";
const privateMachine = "019f0000-0000-7000-8000-000000000003";
const oldInstance = "019f0000-0000-7000-8000-000000000004";
const newInstance = "019f0000-0000-7000-8000-000000000005";
const baselineId = "019f0000-0000-7000-8000-000000000006";
const correlations = [
  "019f0000-0000-7000-8000-000000000007",
  "019f0000-0000-7000-8000-000000000008",
  "019f0000-0000-7000-8000-000000000009",
] as const;
const readIds = [
  "019f0000-0000-7000-8000-00000000000a",
  "019f0000-0000-7000-8000-00000000000b",
] as const;
const workspace = "019f0000-0000-7000-8000-00000000000c";
const areas = [
  { areaId: "019f0000-0000-7000-8000-00000000000d", name: "First batch area" },
  { areaId: "019f0000-0000-7000-8000-00000000000e", name: "Later batch area" },
];
const session = { machineId: installation, endpoint: "codex-local", sessionId: privateMachine };
const caller = { kind: "session", value: session };
const objectId = "a".repeat(64);
const attachment = {
  organizationId: organization,
  machineId: privateMachine,
  installationId: installation,
  enrollmentId: baselineId,
  clientId: baselineId,
  effectiveExpiresAt: 1_900_000_600,
};

function requestFrame(props: {
  readonly correlationId: string;
  readonly requestId: string | null;
  readonly workspaceId: string | null;
  readonly operation: unknown;
}): C2ProductReceipt["readRequest"] {
  return c2ProductReceiptSchema.shape.readRequest.parse({
    kind: "request",
    request: {
      correlationId: props.correlationId,
      caller,
      payload: {
        kind: "workspace",
        request: {
          requestId: props.requestId,
          workspaceId: props.workspaceId,
          operation: props.operation,
        },
      },
    },
  });
}
function reply(correlationId: string, answer: unknown): unknown {
  return {
    kind: "reply",
    correlationId,
    result: { kind: "workspace", reply: { result: { kind: "done", answer }, actors: [] } },
  };
}
function witnessFrames(): {
  readonly before: C2WitnessRecord[];
  readonly after: C2WitnessRecord[];
} {
  const records: C2WitnessRecord[] = [];
  const append = (event: unknown, instanceId = newInstance, phase = "deciding"): void => {
    records.push(c2RecordSchema.parse({ sequence: records.length + 1, phase, instanceId, event }));
  };
  append({ kind: "constructor", objectId }, oldInstance, "connecting");
  append(
    { kind: "upgrade", objectId, instanceId: oldInstance, attachments: [attachment] },
    oldInstance,
    "connecting",
  );
  const before = structuredClone(records);
  append({ kind: "constructor", objectId });
  append({ kind: "handler", objectId, instanceId: newInstance, attachment });
  for (const correlationId of correlations) {
    for (const query of ["byEnrollment", "machineByInstallation", "activeEnrollment"])
      append({ kind: "enrollmentRead", objectId, query, outcome: "read" });
    append({ kind: "restoreResult", objectId, correlationId, outcome: "admitted" });
    append({ kind: "alarmSnapshot", objectId, correlationId, earliestDueMs: null, alarmMs: null });
  }
  return { before, after: records };
}
function fixtureRequests(): Pick<
  C2BatchReceipt,
  "baselineRequest" | "batchRequest" | "readRequests"
> {
  const first = areas[0];
  const last = areas[1];
  assert.ok(first !== undefined && last !== undefined);
  const baseline = requestFrame({
    correlationId: baselineId,
    requestId: baselineId,
    workspaceId: null,
    operation: { kind: "createWorkspace", name: "Batch workspace", description: null },
  });
  const requests = [first, last].map((area, index) => {
    const correlationId = correlations[index === 0 ? 0 : 2];
    assert.ok(correlationId !== undefined);
    return requestFrame({
      correlationId,
      requestId: correlationId,
      workspaceId: workspace,
      operation: { kind: "createArea", name: area.name, description: "Test description" },
    }).request;
  });
  const middle = {
    correlationId: correlations[1],
    caller,
    payload: {
      kind: "mail",
      request: {
        requestId: correlations[1],
        operation: {
          kind: "setWake",
          target: session,
          dueAt: "2030-03-17T17:45:00Z",
          text: "Must be refused",
        },
      },
    },
  };
  const reads = areas.map((area, index) => {
    const correlationId = readIds[index];
    assert.ok(correlationId !== undefined);
    return requestFrame({
      correlationId,
      requestId: null,
      workspaceId: workspace,
      operation: { kind: "show", item: { kind: "area", id: area.areaId } },
    });
  });
  return c2BatchReceiptSchema
    .pick({ baselineRequest: true, batchRequest: true, readRequests: true })
    .parse({
      baselineRequest: baseline,
      batchRequest: { kind: "batch", requests: [requests[0], middle, requests[1]] },
      readRequests: reads,
    });
}

function validReceipt(): C2BatchReceipt {
  const first = areas[0];
  const last = areas[1];
  assert.ok(first !== undefined && last !== undefined);
  const requests = fixtureRequests();
  const witnesses = witnessFrames();
  return c2BatchReceiptSchema.parse({
    kind: "c2BatchHibernation",
    organization,
    installationId: installation,
    welcome: { kind: "welcome", machineId: installation, centralTime: "2030-03-17T17:46:40Z" },
    baselineRequest: requests.baselineRequest,
    baselineReply: reply(baselineId, {
      kind: "workspaceCreated",
      workspace: {
        workspaceId: workspace,
        name: "Batch workspace",
        description: "",
        state: "active",
      },
    }),
    batchRequest: requests.batchRequest,
    batchReplies: [
      reply(correlations[0], {
        kind: "created",
        item: { kind: "area", id: first.areaId },
        position: 1,
      }),
      {
        kind: "reply",
        correlationId: correlations[1],
        result: {
          kind: "mail",
          reply: {
            kind: "refused",
            value: {
              kind: "timingViolation",
              value: { field: "dueAt", requirement: "futureTime" },
            },
          },
        },
      },
      reply(correlations[2], {
        kind: "created",
        item: { kind: "area", id: last.areaId },
        position: 2,
      }),
    ],
    readRequests: requests.readRequests,
    readReplies: areas.map((area, index) => {
      const correlationId = readIds[index];
      assert.ok(correlationId !== undefined);
      return reply(correlationId, {
        kind: "item",
        item: {
          kind: "area",
          item: { ...area, workspaceId: workspace, description: "Test description", state: "open" },
        },
      });
    }),
    beforeEvents: witnesses.before,
    heartbeatEvents: [witnesses.before, witnesses.before, witnesses.before],
    decidingEvents: witnesses.after,
    areas,
    store: {
      columns: ["area_id", "name"],
      rows: areas.map((area) => [
        { kind: "text", value: area.areaId },
        { kind: "text", value: area.name },
      ]),
    },
    wakes: { columns: ["wake_count"], rows: [[{ kind: "integer", value: 0 }]] },
    disposed: true,
  });
}

test("mixed batch binds three independent Replies, later success, two areas and zero wakes", (): void => {
  assert.equal(assertC2BatchReceipt(validReceipt()).areas.length, 2);
});
test("batch receipt rejects reordered replies, changed refusal and unobserved wake effect", (): void => {
  const forged = validReceipt();
  for (const member of [
    forged.baselineRequest.request,
    ...forged.batchRequest.requests,
    ...forged.readRequests.map((frame) => frame.request),
  ])
    member.caller.value.machineId = privateMachine;
  const forgedWake = forged.batchRequest.requests[1];
  assert.ok(forgedWake?.payload.kind === "mail");
  forgedWake.payload.request.operation.target.machineId = privateMachine;
  assert.throws(() => assertC2BatchReceipt(forged), /batch caller uses installation/);
  const reordered = validReceipt();
  reordered.batchReplies.reverse();
  assert.throws(() => assertC2BatchReceipt(reordered), /frame order and correlation/);
  const wrongRefusal = validReceipt();
  const middle = wrongRefusal.batchReplies[1];
  assert.ok(middle?.result.kind === "mail");
  middle.result.reply = {
    kind: "refused",
    value: { kind: "timingViolation", value: { field: "dueAt", requirement: "fiveMinuteGrid" } },
  };
  assert.throws(() => assertC2BatchReceipt(wrongRefusal), /selected FutureTime rule/);
  const wroteWake = validReceipt();
  wroteWake.wakes.rows = [[{ kind: "integer", value: 1 }]];
  assert.throws(() => assertC2BatchReceipt(wroteWake), /zero owning central rows/);
  const lostArea = validReceipt();
  lostArea.store.rows.pop();
  assert.throws(() => assertC2BatchReceipt(lostArea), /exact two owning area rows/);
});
test("batch receipt rejects reuse of earlier member's reads and a second wake", (): void => {
  const reused = validReceipt();
  const firstAlarm = reused.decidingEvents.find(
    (record) =>
      record.event.kind === "alarmSnapshot" && record.event.correlationId === correlations[0],
  );
  const nextRestore = reused.decidingEvents.find(
    (record) =>
      record.event.kind === "restoreResult" && record.event.correlationId === correlations[1],
  );
  assert.ok(firstAlarm !== undefined && nextRestore !== undefined);
  reused.decidingEvents = reused.decidingEvents
    .filter(
      (record) =>
        !(record.sequence > firstAlarm.sequence && record.sequence < nextRestore.sequence),
    )
    .map((record, index) => ({ ...record, sequence: index + 1 }));
  assert.throws(() => assertC2BatchReceipt(reused), /this member's interval/);
  const extra = validReceipt();
  const constructor = extra.decidingEvents.find(
    (record) => record.event.kind === "constructor" && record.instanceId === newInstance,
  );
  assert.ok(constructor !== undefined);
  extra.decidingEvents.push({ ...constructor, sequence: extra.decidingEvents.length + 1 });
  assert.throws(() => assertC2BatchReceipt(extra), /one fresh constructor/);
});

test("batch launcher selects exactly its permanent test and refuses the single journey", (): void => {
  requireSingleC2Selection(`${c2BatchSelector}: test\n`, "batch");
  for (const listing of ["", `${c2ProductSelector}: test`, `${c2BatchSelector}: test\nother: test`])
    assert.throws(() => requireSingleC2Selection(listing, "batch"), /one nonempty test/);
});
