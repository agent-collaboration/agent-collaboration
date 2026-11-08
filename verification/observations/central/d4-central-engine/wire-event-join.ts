/** Join late S1 metadata to original central-facing socket bytes without changing wire order. */
import { z } from "zod";
import { SessionAudit } from "../../../access/session-audit.ts";
import { encode } from "../../../exact-json.ts";
import { decodeWindowTrace, type WindowTraceRecord } from "./window-readback.ts";
import { type D4EventRecord } from "./event-observation.ts";
import { owedSendBindingSchema, type OwedSendBinding } from "./window-observation.ts";

export const wireConnectionBindingSchema = z
  .object({
    physical: z.string().min(1).max(256),
    connection: z.uuidv7(),
    session: z.uuidv7(),
  })
  .strict();
export type WireConnectionBinding = z.infer<typeof wireConnectionBindingSchema>;
const tagged = z.object({ kind: z.string(), value: z.unknown().optional() });
const single = z.object({ frame: z.uuidv7(), body: z.unknown() });
const batch = z
  .array(z.object({ sendId: z.uuidv7(), body: z.unknown() }))
  .min(1)
  .max(32);
interface AuditedConnection {
  readonly server: SessionAudit;
  readonly client: SessionAudit;
  readonly serverId: number;
  readonly clientId: number;
}
interface SourceSend {
  readonly source: Extract<D4EventRecord["sends"][number], { attempt: string }>;
  readonly binding: OwedSendBinding;
  readonly payload: Buffer;
}
export interface JoinedWireEvents {
  readonly trace: string;
  readonly matchedSends: number;
  readonly sourceSendsNotOnWire: readonly string[];
  readonly runtimeQualified: false;
}

export function joinWireEvents(input: {
  readonly trace: string;
  readonly events: readonly D4EventRecord[];
  readonly connections: readonly WireConnectionBinding[];
}): JoinedWireEvents {
  const connections = z.array(wireConnectionBindingSchema).min(1).parse(input.connections);
  if (
    new Set(connections.map((item): string => item.physical)).size !== connections.length ||
    new Set(connections.map((item): string => item.connection)).size !== connections.length
  )
    throw new Error("ambiguous physical/platform connection binding");
  const byPhysical = new Map(
    connections.map((item): [string, WireConnectionBinding] => [item.physical, item]),
  );
  const sourceSends = collectSourceSends(input.events, connections);
  const matched = new Set<string>();
  const audits = new Map<string, AuditedConnection>();
  const output: WindowTraceRecord[] = [];
  for (const record of decodeWindowTrace(input.trace)) {
    if (record.kind === "owedSend") throw new Error("join requires original unbound wire records");
    const connection = byPhysical.get(record.connection);
    if (connection === undefined)
      throw new Error("physical socket has no explicit platform binding");
    output.push(record);
    if (record.kind === "opened") {
      if (audits.has(record.connection)) throw new Error("physical socket identity reused");
      audits.set(record.connection, openAudits(connection, sourceSends, matched));
      for (const expected of sourceSends.values())
        if (expected.source.connection === connection.connection)
          output.push({
            kind: "owedSend",
            connection: record.connection,
            binding: expected.binding,
          });
    } else {
      const audit = audits.get(record.connection);
      if (audit === undefined) throw new Error("wire bytes before physical socket opened");
      if (audit.server.snapshot().active === 0) throw new Error("physical socket already ended");
      if (record.kind === "ended") {
        audit.server.ended(audit.serverId);
        audit.client.ended(audit.clientId);
      } else {
        const bytes = Buffer.from(record.bytes, "base64");
        if (bytes.toString("base64") !== record.bytes)
          throw new Error("noncanonical original byte record");
        if (record.kind === "server") audit.server.receive(audit.serverId, bytes);
        else audit.client.receive(audit.clientId, bytes);
      }
    }
  }
  for (const audit of audits.values()) {
    audit.server.requireComplete();
    audit.client.requireComplete();
    if (audit.server.snapshot().welcomes < 1 || (audit.client.snapshot().frames.hello ?? 0) < 1)
      throw new Error("wire join lacks actual Hello/Welcome");
  }
  return {
    trace: `${output.map(encode).join("\n")}\n`,
    matchedSends: matched.size,
    sourceSendsNotOnWire: [...sourceSends.keys()].filter((send): boolean => !matched.has(send)),
    runtimeQualified: false,
  };
}

function collectSourceSends(
  events: readonly D4EventRecord[],
  connections: readonly WireConnectionBinding[],
): Map<string, SourceSend> {
  const platformConnections = new Set(connections.map((item): string => item.connection));
  const sourceSends = new Map<string, SourceSend>();
  for (const event of events) {
    for (const send of event.sends) {
      // Slice 2b retains raw sends with all owed identifiers explicitly null.
      // Their bytes are observable, but cannot supply an owed-window binding.
      if (send.attempt === null) continue;
      if (
        !platformConnections.has(send.connection) ||
        !["push", "publishStatus"].includes(send.requestKind)
      )
        continue;
      if (event.object.kind !== "fleet") throw new Error("socket send attributed to a Copy object");
      if (sourceSends.has(send.sendId)) throw new Error("duplicate S1 send UUID");
      const kind = send.requestKind === "push" ? "push" : "status";
      if (send.attemptKind !== kind) throw new Error("S1 request/attempt kind mismatch");
      sourceSends.set(send.sendId, {
        source: send,
        binding: owedSendBindingSchema.parse({
          attempt: send.attempt,
          subject: send.subject,
          sendId: send.sendId,
          kind,
        }),
        payload: Buffer.from(send.frame, "base64"),
      });
    }
  }
  return sourceSends;
}

function observeServer(
  connection: WireConnectionBinding,
  raw: unknown,
  payload: Buffer,
  sourceSends: ReadonlyMap<string, SourceSend>,
  matched: Set<string>,
): void {
  const frame = tagged.parse(raw);
  if (frame.kind === "welcome") {
    if (z.object({ sessionId: z.uuidv7() }).parse(frame.value).sessionId !== connection.session)
      throw new Error("Welcome does not match its actual connection/session binding");
    return;
  }
  const items =
    frame.kind === "request"
      ? [single.parse(frame.value)].map((item) => ({ sendId: item.frame, body: item.body }))
      : frame.kind === "requestBatch"
        ? batch.parse(frame.value)
        : [];
  let batchSendId: string | undefined;
  for (const [index, item] of items.entries()) {
    const owner = tagged.parse(item.body);
    if (owner.kind !== "crate") continue;
    const request = tagged.parse(owner.value);
    if (!["push", "publishStatus"].includes(request.kind)) continue;
    const expected = sourceSends.get(item.sendId);
    if (
      expected === undefined ||
      expected.source.connection !== connection.connection ||
      expected.source.session !== connection.session
    )
      throw new Error("wire send has no matching S1 connection/session identity");
    if (
      matched.has(item.sendId) ||
      !expected.payload.equals(payload) ||
      expected.source.requestKind !== request.kind
    )
      throw new Error("wire send replay or exact S1 payload mismatch");
    if (frame.kind === "requestBatch") {
      const grouping = expected.source.batch;
      if (
        grouping === null ||
        grouping.index !== BigInt(index) ||
        grouping.items !== BigInt(items.length) ||
        (batchSendId !== undefined && batchSendId !== grouping.batchSendId)
      )
        throw new Error("S1 batch index/count does not match original wire");
      batchSendId = grouping.batchSendId;
    } else if (expected.source.batch !== null)
      throw new Error("S1 labeled a single frame as a batch");
    if (request.kind === "push") {
      const subject = z
        .object({ item: z.object({ value: z.object({ id: z.uuidv7() }) }) })
        .parse(request.value).item.value.id;
      if (subject !== expected.binding.subject) throw new Error("Push subject differs from S1");
    } else {
      const version = z
        .object({ publication: z.object({ version: z.bigint().nonnegative() }) })
        .parse(request.value).publication.version;
      if (version !== expected.source.sentVersion)
        throw new Error("publication version differs from S1");
    }
    matched.add(item.sendId);
  }
}

function openAudits(
  connection: WireConnectionBinding,
  sourceSends: ReadonlyMap<string, SourceSend>,
  matched: Set<string>,
): AuditedConnection {
  const server = new SessionAudit(
    (value, payload): void => observeServer(connection, value, payload, sourceSends, matched),
    "server",
    "batches",
  );
  const client = new SessionAudit(
    (raw): void => {
      const frame = tagged.parse(raw);
      if (
        frame.kind === "hello" &&
        z.object({ sessionId: z.uuidv7() }).parse(frame.value).sessionId !== connection.session
      )
        throw new Error("Hello differs from actual connection/session binding");
    },
    "client",
    "batches",
  );
  return {
    server,
    client,
    serverId: server.opened(),
    clientId: client.opened(),
  };
}
