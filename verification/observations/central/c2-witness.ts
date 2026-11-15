// External witnesses for the real compiled CentralObject. No domain effects or tokens.
import assert from "node:assert/strict";
import { randomUUIDv7 } from "node:crypto";
import { Response, type V4FetchHandler } from "miniflare";
import { z } from "zod";

export const c2PhaseSchema = z.enum([
  "connecting",
  "baseline",
  "evicting",
  "heartbeat",
  "deciding",
  "settling",
]);
const objectIdSchema = z.string().regex(/^[0-9a-f]{64}$/);
const instanceIdSchema = z.uuidv7();
export const c2AttachmentSchema = z
  .object({
    organizationId: z.uuidv7(),
    machineId: z.uuidv7(),
    installationId: z.uuidv7(),
    enrollmentId: z.uuidv7(),
    clientId: z.uuidv7(),
    effectiveExpiresAt: z.number().int().safe().nonnegative(),
  })
  .strict();
const common = z.object({ objectId: objectIdSchema });
const milliseconds = z.number().int().safe().nonnegative().nullable();
export const c2EventSchema = z.discriminatedUnion("kind", [
  common.extend({ kind: z.literal("constructor") }).strict(),
  common
    .extend({
      kind: z.literal("handler"),
      instanceId: instanceIdSchema,
      attachment: c2AttachmentSchema,
    })
    .strict(),
  common
    .extend({
      kind: z.literal("upgrade"),
      instanceId: instanceIdSchema,
      attachments: z.array(c2AttachmentSchema),
    })
    .strict(),
  common
    .extend({
      kind: z.literal("enrollmentRead"),
      query: z.enum(["byEnrollment", "machineByInstallation", "activeEnrollment"]),
      outcome: z.enum(["read", "unavailable"]),
    })
    .strict(),
  common
    .extend({
      kind: z.literal("restoreResult"),
      correlationId: z.uuidv7(),
      outcome: z.enum(["admitted", "refused", "unavailable"]),
    })
    .strict(),
  common
    .extend({
      kind: z.literal("alarmSnapshot"),
      correlationId: z.uuidv7(),
      earliestDueMs: milliseconds,
      alarmMs: milliseconds,
    })
    .strict(),
]);
export const c2RecordSchema = z
  .object({
    sequence: z.number().int().positive(),
    phase: c2PhaseSchema,
    instanceId: instanceIdSchema,
    event: c2EventSchema,
  })
  .strict();
export type C2WitnessRecord = z.infer<typeof c2RecordSchema>;

export class C2WitnessRecorder {
  phase: z.infer<typeof c2PhaseSchema> = "connecting";
  private readonly records: C2WitnessRecord[] = [];
  private readonly instances = new Map<string, string>();

  snapshot(): readonly C2WitnessRecord[] {
    return structuredClone(this.records);
  }

  readonly observe: V4FetchHandler = async (request): Promise<Response> => {
    if (request.method !== "POST" || request.url !== "https://c2-witness.test/record")
      return new Response("invalid witness request", { status: 400 });
    // Do not return parser diagnostics containing caller data.
    let input: unknown;
    try {
      input = await request.json();
    } catch {
      return new Response(null, { status: 400 });
    }
    const parsed = c2EventSchema.safeParse(input);
    if (!parsed.success) return new Response(null, { status: 400 });
    const event = parsed.data;
    const instanceId =
      event.kind === "constructor" ? randomUUIDv7() : this.instances.get(event.objectId);
    if (instanceId === undefined) return new Response(null, { status: 409 });
    if ("instanceId" in event && event.instanceId !== instanceId)
      return new Response(null, { status: 409 });
    this.instances.set(event.objectId, instanceId);
    this.records.push({ sequence: this.records.length + 1, phase: this.phase, instanceId, event });
    return event.kind === "constructor"
      ? Response.json({ instanceId })
      : new Response(null, { status: 204 });
  };
}

// The wrapper delegates the compiled class; finite witness callbacks finish before
// replies. Its own hibernation is a runtime obligation, never inferred from this text.
export const c2WitnessWrapperSource = `
import * as original from './index.js';
export * from './index.js';
export default original.default;
export class CentralObject extends original.CentralObject {
  constructor(state, env) {
    super(state, env);
    this.witnessState = state;
    this.witnessEnv = env;
    this.witnessReady = state.blockConcurrencyWhile(async () => {
      const response = await env.TEST_C2_WITNESS.fetch('https://c2-witness.test/record', {
        method: 'POST', body: JSON.stringify({kind:'constructor', objectId:state.id.toString()})
      });
      if (!response.ok) throw new Error('constructor witness refused');
      this.witnessInstance = (await response.json()).instanceId;
    });
  }
  async recordWitness(event) {
    await this.witnessReady;
    const response = await this.witnessEnv.TEST_C2_WITNESS.fetch('https://c2-witness.test/record', {
      method: 'POST', body: JSON.stringify({...event,
        objectId:this.witnessState.id.toString(), instanceId:this.witnessInstance})
    });
    if (response.status !== 204) throw new Error('object witness refused');
  }
  async fetch(request) {
    await this.witnessReady;
    const response = await super.fetch(request);
    if (response.status === 101) await this.recordWitness({kind:'upgrade',
      attachments:this.witnessState.getWebSockets().map(socket => socket.deserializeAttachment())});
    return response;
  }
  async webSocketMessage(socket, message) {
    await this.recordWitness({kind:'handler', attachment:socket.deserializeAttachment()});
    return super.webSocketMessage(socket, message);
  }
}
`;

/** Require real SQL I/O followed by typed restoration on the serving new instance. */
export function assertC2Restore(input: {
  readonly records: readonly C2WitnessRecord[];
  readonly correlationId: string;
  readonly before: readonly C2WitnessRecord[];
  readonly attachment: z.infer<typeof c2AttachmentSchema>;
}): void {
  const fresh = input.records.slice(input.before.length);
  const constructors = fresh.filter((record) => record.event.kind === "constructor");
  const handlers = fresh.filter((record) => record.event.kind === "handler");
  assert.equal(constructors.length, 1, "deciding frame needs one fresh constructor");
  assert.equal(handlers.length, 1, "deciding frame needs one semantic handler");
  const constructor = constructors[0];
  const handler = handlers[0];
  assert.ok(constructor !== undefined && handler !== undefined);
  assert.equal(constructor.phase, "deciding", "constructor witness must arrive while deciding");
  assert.equal(handler.phase, "deciding", "handler witness must arrive while deciding");
  assert.ok(constructor.sequence < handler.sequence, "handler follows fresh constructor");
  const originalObject = input.before.find((record) => record.event.kind === "constructor")?.event
    .objectId;
  assert.equal(
    constructor.event.objectId,
    originalObject,
    "same physical organization object must reconstruct",
  );
  assert.equal(
    handler.event.objectId,
    originalObject,
    "handler uses the original organization object",
  );
  assert.equal(handler.instanceId, constructor.instanceId, "handler must use new instance");
  assert.ok(
    !input.before.some((record) => record.instanceId === constructor.instanceId),
    "serving instance must be new",
  );
  assert.ok(handler.event.kind === "handler");
  assert.deepEqual(handler.event.attachment, input.attachment, "original attachment must restore");
  const restorations = fresh.filter(
    (record) =>
      record.event.kind === "restoreResult" && record.event.correlationId === input.correlationId,
  );
  assert.equal(restorations.length, 1, "matching restoration witness required exactly once");
  const restored = restorations[0];
  assert.ok(restored !== undefined, "matching restoration witness required");
  assert.equal(restored.instanceId, constructor.instanceId);
  assert.ok(restored.event.kind === "restoreResult");
  assert.equal(restored.event.outcome, "admitted", "live restoration must admit");
  const reads = fresh.filter(
    (record) =>
      record.sequence > handler.sequence &&
      record.sequence < restored.sequence &&
      record.instanceId === constructor.instanceId &&
      record.event.kind === "enrollmentRead" &&
      record.event.outcome === "read",
  );
  const queries = reads.map((record) =>
    record.event.kind === "enrollmentRead" ? record.event.query : "",
  );
  const enrollmentIndex = queries.indexOf("byEnrollment");
  const machineIndex = queries.indexOf("machineByInstallation", enrollmentIndex + 1);
  const activeIndex = queries.indexOf("activeEnrollment", machineIndex + 1);
  assert.ok(
    enrollmentIndex >= 0 && machineIndex > enrollmentIndex && activeIndex > machineIndex,
    "all live enrollment queries must complete",
  );
  const alarms = fresh.filter(
    (record) =>
      record.event.kind === "alarmSnapshot" && record.event.correlationId === input.correlationId,
  );
  assert.equal(alarms.length, 1, "matching alarm snapshot required exactly once");
  const alarm = alarms[0];
  assert.ok(
    alarm !== undefined && alarm.event.kind === "alarmSnapshot",
    "matching actual alarm readback required",
  );
  assert.equal(alarm.instanceId, constructor.instanceId);
  assert.ok(alarm.sequence > restored.sequence, "alarm readback follows restoration");
  assert.equal(
    alarm.event.alarmMs,
    alarm.event.earliestDueMs,
    "platform alarm must match EARLIEST_DUE",
  );
}
