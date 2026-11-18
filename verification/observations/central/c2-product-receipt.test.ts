import assert from "node:assert/strict";
import { test } from "node:test";
import {
  assertC2ProductReceipt,
  c2ProductReceiptSchema,
  type C2ProductReceipt,
} from "./c2-product-receipt.ts";
import { c2ProductSelector, requireSingleC2Selection } from "./c2-product-run.ts";
import { c2RecordSchema, type C2WitnessRecord } from "./c2-witness.ts";

const organization = "019f0000-0000-7000-8000-000000000001";
const installation = "019f0000-0000-7000-8000-000000000002";
const privateMachine = "019f0000-0000-7000-8000-000000000003";
const oldInstance = "019f0000-0000-7000-8000-000000000004";
const newInstance = "019f0000-0000-7000-8000-000000000005";
const baselineId = "019f0000-0000-7000-8000-000000000006";
const decidingId = "019f0000-0000-7000-8000-000000000007";
const readId = "019f0000-0000-7000-8000-000000000008";
const workspace = "019f0000-0000-7000-8000-000000000009";
const areaId = "019f0000-0000-7000-8000-00000000000a";
const name = "Hibernation area";
const objectId = "a".repeat(64);
const attachment = {
  organizationId: organization,
  machineId: privateMachine,
  installationId: installation,
  enrollmentId: baselineId,
  clientId: baselineId,
  effectiveExpiresAt: 1_800_000_600,
};
const caller = {
  kind: "session",
  value: { machineId: installation, endpoint: "codex-local", sessionId: privateMachine },
};

function events(instanceId: string, correlationId: string, offset: number): C2WitnessRecord[] {
  const raw = [
    { kind: "constructor", objectId },
    ...(offset === 0 ? [{ kind: "upgrade", objectId, instanceId, attachments: [attachment] }] : []),
    { kind: "handler", objectId, instanceId, attachment },
    { kind: "enrollmentRead", objectId, query: "byEnrollment", outcome: "read" },
    { kind: "enrollmentRead", objectId, query: "machineByInstallation", outcome: "read" },
    { kind: "enrollmentRead", objectId, query: "activeEnrollment", outcome: "read" },
    { kind: "restoreResult", objectId, correlationId, outcome: "admitted" },
    { kind: "alarmSnapshot", objectId, correlationId, earliestDueMs: null, alarmMs: null },
  ];
  return raw.map((event, index) =>
    c2RecordSchema.parse({
      sequence: offset + index + 1,
      phase: offset === 0 ? "baseline" : "deciding",
      instanceId,
      event,
    }),
  );
}
function frame(props: {
  readonly correlationId: string;
  readonly operation: unknown;
  readonly requestId: string | null;
  readonly workspaceId: string | null;
}): unknown {
  return {
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
  };
}
function reply(correlationId: string, answer: unknown): unknown {
  return {
    kind: "reply",
    correlationId,
    result: { kind: "workspace", reply: { result: { kind: "done", answer }, actors: [] } },
  };
}
function validReceipt(): C2ProductReceipt {
  const beforeEvents = events(oldInstance, baselineId, 0);
  const area = { kind: "area", id: areaId };
  return c2ProductReceiptSchema.parse({
    kind: "c2Hibernation",
    organization,
    installationId: installation,
    welcome: { kind: "welcome", machineId: installation, centralTime: "2027-01-15T08:00:00Z" },
    baselineRequest: frame({
      correlationId: baselineId,
      operation: { kind: "createWorkspace", name: "Baseline workspace", description: null },
      requestId: baselineId,
      workspaceId: null,
    }),
    baselineReply: reply(baselineId, {
      kind: "workspaceCreated",
      workspace: {
        workspaceId: workspace,
        name: "Baseline workspace",
        description: "",
        state: "active",
      },
    }),
    decidingRequest: frame({
      correlationId: decidingId,
      operation: { kind: "createArea", name, description: "Test description" },
      requestId: baselineId,
      workspaceId: workspace,
    }),
    decidingReply: reply(decidingId, { kind: "created", item: area, position: 1 }),
    readRequest: frame({
      correlationId: readId,
      operation: { kind: "show", item: area },
      requestId: null,
      workspaceId: workspace,
    }),
    readReply: reply(readId, {
      kind: "item",
      item: {
        kind: "area",
        item: {
          areaId,
          workspaceId: workspace,
          name,
          description: "Test description",
          state: "open",
        },
      },
    }),
    beforeEvents,
    heartbeatEvents: [beforeEvents, beforeEvents, beforeEvents],
    decidingEvents: [...beforeEvents, ...events(newInstance, decidingId, beforeEvents.length)],
    area: { areaId, name },
    store: {
      columns: ["area_id", "name"],
      rows: [
        [
          { kind: "text", value: areaId },
          { kind: "text", value: name },
        ],
      ],
    },
    disposed: true,
  });
}

test("C2 actual-frame receipt binds CreateArea, read, owning row and same-socket witnesses", (): void => {
  assert.equal(assertC2ProductReceipt(validReceipt()).area.name, name);
});

test("C2 baseline accepts a CreateWorkspace description as null or text, and the reply's description", (): void => {
  const described = validReceipt();
  described.baselineRequest.request.payload.request.operation = {
    kind: "createWorkspace",
    name: "Baseline workspace",
    description: "Team space",
  };
  described.baselineReply.result.reply.result = {
    kind: "done",
    answer: {
      kind: "workspaceCreated",
      workspace: {
        workspaceId: workspace,
        name: "Baseline workspace",
        description: "Team space",
        state: "active",
      },
    },
  };
  assert.equal(assertC2ProductReceipt(described).area.name, name);
  assert.equal(assertC2ProductReceipt(validReceipt()).area.name, name);
  const missing = validReceipt();
  missing.baselineReply.result.reply.result = {
    kind: "done",
    answer: {
      kind: "workspaceCreated",
      workspace: { workspaceId: workspace, name: "Baseline workspace", state: "active" },
    },
  };
  assert.throws(() => assertC2ProductReceipt(missing));
});

test("C2 area read requires the stored description and matches it to CreateArea", (): void => {
  const withoutDescription = validReceipt();
  withoutDescription.readReply.result.reply.result = {
    kind: "done",
    answer: {
      kind: "item",
      item: { kind: "area", item: { areaId, workspaceId: workspace, name, state: "open" } },
    },
  };
  assert.throws(() => assertC2ProductReceipt(withoutDescription));
  const otherDescription = validReceipt();
  otherDescription.readReply.result.reply.result = {
    kind: "done",
    answer: {
      kind: "item",
      item: {
        kind: "area",
        item: { areaId, workspaceId: workspace, name, description: "Other text", state: "open" },
      },
    },
  };
  assert.throws(() => assertC2ProductReceipt(otherDescription), /stored description/);
});

test("C2 receipt rejects wrong correlation, fake store success and missing actual read", (): void => {
  const wrongCorrelation = validReceipt();
  wrongCorrelation.decidingReply.correlationId = readId;
  assert.throws(() => assertC2ProductReceipt(wrongCorrelation), /Reply must match/);
  const wrongStore = validReceipt();
  wrongStore.store.rows = [
    [
      { kind: "text", value: areaId },
      { kind: "text", value: "unrelated area" },
    ],
  ];
  assert.throws(() => assertC2ProductReceipt(wrongStore), /unrelated area/);
  const wrongRead = validReceipt();
  wrongRead.readReply.result.reply.result = { kind: "done", answer: { kind: "page", items: [] } };
  assert.throws(() => assertC2ProductReceipt(wrongRead), /invalid_value/);
  const wrongWorkspace = validReceipt();
  wrongWorkspace.baselineReply.result.reply.result = {
    kind: "done",
    answer: {
      kind: "workspaceCreated",
      workspace: {
        workspaceId: areaId,
        name: "Baseline workspace",
        description: "",
        state: "active",
      },
    },
  };
  assert.throws(() => assertC2ProductReceipt(wrongWorkspace), /actual baseline workspace/);
});

test("C2 receipt rejects heartbeat wake, skipped SQL witness, altered attachment and missing disposal", (): void => {
  const wake = validReceipt();
  wake.heartbeatEvents[1] = wake.decidingEvents;
  assert.throws(() => assertC2ProductReceipt(wake), /each pong/);
  const missing = validReceipt();
  missing.decidingEvents = missing.decidingEvents.filter(
    (record) =>
      !(
        record.sequence > missing.beforeEvents.length &&
        record.event.kind === "enrollmentRead" &&
        record.event.query === "activeEnrollment"
      ),
  );
  assert.throws(() => assertC2ProductReceipt(missing), /all live enrollment queries/);
  const changed = validReceipt();
  const handler = changed.decidingEvents.find(
    (record) => record.instanceId === newInstance && record.event.kind === "handler",
  );
  assert.ok(handler?.event.kind === "handler");
  handler.event.attachment.effectiveExpiresAt += 1;
  assert.throws(() => assertC2ProductReceipt(changed), /original attachment/);
  assert.throws(
    () => assertC2ProductReceipt({ ...validReceipt(), disposed: false }),
    /invalid_value/,
  );
});

test("C2 receipt rejects other-object reconstruction, duplicate restoration and invented due work", (): void => {
  const otherObject = validReceipt();
  const constructor = otherObject.decidingEvents.find(
    (record) => record.instanceId === newInstance && record.event.kind === "constructor",
  );
  assert.ok(constructor !== undefined);
  constructor.event.objectId = "b".repeat(64);
  assert.throws(() => assertC2ProductReceipt(otherObject), /same physical organization/);
  const nonNullAlarm = validReceipt();
  for (const record of nonNullAlarm.decidingEvents) {
    if (record.instanceId === newInstance && record.event.kind === "alarmSnapshot") {
      record.event.alarmMs = 123;
      record.event.earliestDueMs = 123;
    }
  }
  assert.throws(() => assertC2ProductReceipt(nonNullAlarm), /no due work/);
  const duplicate = validReceipt();
  const restoration = duplicate.decidingEvents.find(
    (record) => record.instanceId === newInstance && record.event.kind === "restoreResult",
  );
  assert.ok(restoration !== undefined);
  duplicate.decidingEvents.push({ ...restoration, sequence: 100 });
  assert.throws(
    () => assertC2ProductReceipt(duplicate),
    /restoration witness required exactly once/,
  );
});

test("C2 exact selector refuses zero matches, a wrong test and extra discovered cases", (): void => {
  requireSingleC2Selection(`${c2ProductSelector}: test\n`);
  for (const listing of ["", "unrelated: test", `${c2ProductSelector}: test\nother: test`])
    assert.throws(() => requireSingleC2Selection(listing), /one nonempty test/);
});
