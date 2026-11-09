/** Bind one live physical stream to stable owner registration reads, never to opening order. */
import { z } from "zod";
import { SessionAudit } from "../../../access/session-audit.ts";
import { decodeWindowTrace } from "./window-readback.ts";
import { wireConnectionBindingSchema, type WireConnectionBinding } from "./wire-event-join.ts";

const admissionSchema = z.array(
  z.object({
    connection: z.uuidv7(),
    peer: z.uuidv7(),
    registration: z.object({ kind: z.string(), sessionId: z.uuidv7().optional() }).passthrough(),
  }),
);
export class LiveSessionBindingPending extends Error {}

function attachedOwner(raw: unknown, peer: string): { connection: string; session: string } {
  const attached = admissionSchema
    .parse(raw)
    .filter((row): boolean => row.peer === peer && row.registration.kind === "attached");
  if (attached.length !== 1 || attached[0] === undefined)
    throw new LiveSessionBindingPending("one attached owner registration required");
  const session = z.uuidv7().parse(attached[0].registration.sessionId);
  return { connection: attached[0].connection, session };
}

interface PhysicalStream {
  readonly client: SessionAudit;
  readonly server: SessionAudit;
  readonly clientId: number;
  readonly serverId: number;
  active: boolean;
  hello: string | undefined;
  welcome: string | undefined;
}
const envelope = z.object({ kind: z.string(), value: z.unknown() });
function openStream(): PhysicalStream {
  const sessions: { hello: string | undefined; welcome: string | undefined } = {
    hello: undefined,
    welcome: undefined,
  };
  const sessionFrom = (raw: unknown, kind: "hello" | "welcome"): void => {
    const frame = envelope.parse(raw);
    if (frame.kind !== kind) return;
    const session = z.object({ sessionId: z.uuidv7() }).parse(frame.value).sessionId;
    if (sessions[kind] !== undefined && sessions[kind] !== session)
      throw new Error("session changed on one physical stream");
    sessions[kind] = session;
  };
  const client = new SessionAudit(
    (frame): void => sessionFrom(frame, "hello"),
    "client",
    "batches",
  );
  const server = new SessionAudit(
    (frame): void => sessionFrom(frame, "welcome"),
    "server",
    "batches",
  );
  return {
    client,
    server,
    clientId: client.opened(),
    serverId: server.opened(),
    active: true,
    get hello(): string | undefined {
      return sessions.hello;
    },
    get welcome(): string | undefined {
      return sessions.welcome;
    },
  };
}

/** The caller owns the peer-scoped gate and retains both actual admission replies. */
export function bindLiveSession(input: {
  readonly trace: string;
  readonly peer: string;
  readonly admissionBefore: unknown;
  readonly admissionAfter: unknown;
}): WireConnectionBinding {
  z.uuidv7().parse(input.peer);
  const before = attachedOwner(input.admissionBefore, input.peer),
    after = attachedOwner(input.admissionAfter, input.peer);
  if (before.connection !== after.connection || before.session !== after.session)
    throw new LiveSessionBindingPending("owner registration changed across the wire read");
  const streams = new Map<string, PhysicalStream>();
  for (const record of decodeWindowTrace(input.trace)) {
    if (record.kind === "opened") {
      if (streams.has(record.connection)) throw new Error("physical stream label reused");
      streams.set(record.connection, openStream());
      continue;
    }
    const stream = streams.get(record.connection);
    if (stream === undefined || !stream.active)
      throw new Error("wire bytes outside an open physical stream");
    if (record.kind === "ended") {
      stream.client.ended(stream.clientId);
      stream.server.ended(stream.serverId);
      stream.active = false;
    } else if (record.kind === "client" || record.kind === "server") {
      const bytes = Buffer.from(record.bytes, "base64");
      if (bytes.toString("base64") !== record.bytes) throw new Error("noncanonical original bytes");
      (record.kind === "client" ? stream.client : stream.server).receive(
        record.kind === "client" ? stream.clientId : stream.serverId,
        bytes,
      );
    } else throw new Error("binding requires original unbound wire records");
  }
  const live = [...streams].filter(([, stream]): boolean => stream.active);
  if (live.length !== 1 || live[0] === undefined)
    throw new LiveSessionBindingPending("one live peer-scoped physical stream required");
  const [physical, stream] = live[0];
  if (
    stream.client.snapshot().invalidConnections > 0 ||
    stream.server.snapshot().invalidConnections > 0
  )
    throw new Error("invalid original Session bytes");
  try {
    stream.client.requireComplete();
    stream.server.requireComplete();
  } catch {
    throw new LiveSessionBindingPending("original Session message is incomplete");
  }
  if (stream.hello === undefined || stream.welcome === undefined)
    throw new LiveSessionBindingPending("actual Hello and Welcome required");
  if (stream.hello !== after.session || stream.welcome !== after.session)
    throw new Error("wire Hello/Welcome differs from actual owner registration");
  return wireConnectionBindingSchema.parse({
    physical,
    connection: after.connection,
    session: after.session,
  });
}
