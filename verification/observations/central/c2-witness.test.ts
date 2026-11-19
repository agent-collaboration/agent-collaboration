import assert from "node:assert/strict";
import { test } from "node:test";
import { once } from "node:events";
import { mkdir, writeFile, rm } from "node:fs/promises";
import { join } from "node:path";
import WebSocket from "ws";
import { WorkerdStorage } from "../../targets/workerd-storage.ts";
import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import { c2WitnessQualificationSource } from "../../test-support/c2-witness-worker.ts";
import {
  C2WitnessRecorder,
  assertC2Restore,
  c2AttachmentSchema,
  c2RecordSchema,
  type C2WitnessRecord,
} from "./c2-witness.ts";

const organization = "019f0000-0000-7000-8000-000000000001";
const instance = "019f0000-0000-7000-8000-000000000002";
const oldInstance = "019f0000-0000-7000-8000-000000000003";
const correlation = "019f0000-0000-7000-8000-000000000004";
const attachment = c2AttachmentSchema.parse({
  organizationId: organization,
  machineId: "019f0000-0000-7000-8000-000000000005",
  enrollmentId: "019f0000-0000-7000-8000-000000000007",
  clientId: "019f0000-0000-7000-8000-000000000008",
  effectiveExpiresAt: 1_800_000_600,
});
const objectId = "a".repeat(64);
const before: C2WitnessRecord[] = [
  {
    sequence: 1,
    phase: "connecting",
    instanceId: oldInstance,
    event: { kind: "constructor", objectId },
  },
];
function validRecords(): C2WitnessRecord[] {
  return [
    ...before,
    ...[
      { kind: "constructor", objectId },
      { kind: "handler", objectId, instanceId: instance, attachment },
      { kind: "enrollmentRead", objectId, query: "byEnrollment", outcome: "read" },
      { kind: "enrollmentRead", objectId, query: "machineById", outcome: "read" },
      { kind: "enrollmentRead", objectId, query: "activeEnrollment", outcome: "read" },
      { kind: "restoreResult", objectId, correlationId: correlation, outcome: "admitted" },
      // An actual held recheck may legitimately repeat these reads.
      { kind: "enrollmentRead", objectId, query: "byEnrollment", outcome: "read" },
      {
        kind: "alarmSnapshot",
        objectId,
        correlationId: correlation,
        earliestDueMs: null,
        alarmMs: null,
      },
    ].map((event, index) => {
      // Parse external fixture values through the same wire schema.
      return c2RecordSchema.parse({
        sequence: index + 2,
        phase: "deciding",
        instanceId: instance,
        event,
      });
    }),
  ];
}

test("C2 attachment refuses the former installation ID field", (): void => {
  assert.equal(
    c2AttachmentSchema.safeParse({ ...attachment, installationId: organization }).success,
    false,
  );
});

test("C2 oracle accepts real read set plus repeated held recheck", (): void => {
  assertC2Restore({ records: validRecords(), before, attachment, correlationId: correlation });
});

test("C2 oracle rejects missing live SQL, stale instance, attachment drift and wrong alarm", (): void => {
  const missing = validRecords().filter(
    (record) =>
      !(record.event.kind === "enrollmentRead" && record.event.query === "activeEnrollment"),
  );
  assert.throws(
    () => assertC2Restore({ records: missing, before, attachment, correlationId: correlation }),
    /all live enrollment queries/,
  );
  const stale = validRecords().map((record) => ({ ...record, instanceId: oldInstance }));
  assert.throws(
    () => assertC2Restore({ records: stale, before, attachment, correlationId: correlation }),
    /must be new/,
  );
  const wrongAttachment = { ...attachment, machineId: organization };
  assert.throws(
    () =>
      assertC2Restore({
        records: validRecords(),
        before,
        attachment: wrongAttachment,
        correlationId: correlation,
      }),
    /original attachment/,
  );
  const wrongAlarm = validRecords().map((record) =>
    record.event.kind === "alarmSnapshot"
      ? { ...record, event: { ...record.event, alarmMs: 123 } }
      : record,
  );
  assert.throws(
    () => assertC2Restore({ records: wrongAlarm, before, attachment, correlationId: correlation }),
    /platform alarm/,
  );
  const extraHandler = validRecords();
  const handler = extraHandler.find((record) => record.event.kind === "handler");
  assert.ok(handler !== undefined);
  extraHandler.push({ ...handler, sequence: 100 });
  assert.throws(
    () =>
      assertC2Restore({ records: extraHandler, before, attachment, correlationId: correlation }),
    /one semantic handler/,
  );
});

test("compiled-class wrapper shape hibernates and delegates on the original workerd socket", async (): Promise<void> => {
  const root = createFixtureDirectory("c2-witness-wrapper");
  await mkdir(join(root, "bundle"));
  await writeFile(join(root, "bundle/index.js"), c2WitnessQualificationSource);
  await writeFile(join(root, "bundle/index_bg.wasm"), Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]));
  const witness = new C2WitnessRecorder();
  let clock = 1_800_000_000_000;
  const host = await WorkerdStorage.start(
    {
      bundleDirectory: join(root, "bundle"),
      buildCommand: "permanent apparatus fixture",
      compatibilityDate: "2026-08-21",
      durableObjects: { CENTRAL: "CentralObject" },
      textBindings: { ATTACHMENT: JSON.stringify(attachment) },
      c2Witness: witness,
    },
    undefined,
    (): number => clock,
  );
  const socket = new WebSocket(host.origin.replace("http:", "ws:") + "/" + organization);
  const welcome = once(socket, "message");
  try {
    await once(socket, "open");
    const [text] = await welcome;
    assert.equal(String(text), "welcome");
    const initial = witness.snapshot();
    assert.deepEqual(
      initial.map((record) => record.event.kind),
      ["constructor", "upgrade"],
    );
    witness.phase = "evicting";
    await host.evictCentral(organization);
    assert.deepEqual(witness.snapshot(), initial, "eviction itself must not reconstruct");
    clock += 1000; // One external cell, no target fetch.
    witness.phase = "heartbeat";
    for (let index = 0; index < 3; index += 1) {
      const pong = once(socket, "message");
      socket.send("ping");
      assert.equal(String((await pong)[0]), "pong");
      assert.deepEqual(witness.snapshot(), initial, "heartbeat must add no witnesses");
    }
    witness.phase = "deciding";
    const reply = once(socket, "message");
    socket.send("actual-delegation-probe");
    assert.deepEqual(JSON.parse(String((await reply)[0])), {
      delegated: "actual-delegation-probe",
      clock: { milliseconds: clock },
    });
    const final = witness.snapshot();
    assert.deepEqual(
      final.map((record) => record.event.kind),
      ["constructor", "upgrade", "constructor", "handler"],
    );
    assert.notEqual(final[0]?.instanceId, final[2]?.instanceId);
    assert.equal(final[2]?.phase, "deciding");
    const handler = final[3]?.event;
    assert.ok(handler?.kind === "handler");
    assert.deepEqual(handler.attachment, attachment);
  } finally {
    const closed = once(socket, "close");
    socket.close(1000, "complete");
    await closed;
    await host.close();
    await rm(root, { recursive: true });
  }
});
