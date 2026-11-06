import assert from "node:assert/strict";
import { test } from "node:test";
import { SessionAudit } from "./session-audit.ts";

const itemId = "01900000-0000-7000-8000-000000000001";
const cancelId = "01900000-0000-7000-8000-000000000005";
const frameId = "019eef00-0000-7000-8000-000000000004";
const peerId = "019eef00-0000-7000-8000-000000000002";

// Literal tags and nesting follow fleet-sync-protocol push_wire.rs at 7b69ca0.
function pushFrame(kind: "item" | "cancel" = "item", id = itemId): unknown {
  return {
    kind: "request",
    value: {
      frame: frameId,
      body: {
        kind: "crate",
        value: {
          kind: "push",
          value: {
            call: { operation: "019eef00-0000-7000-8000-000000000003", step: "push" },
            item: {
              kind,
              value: {
                id,
                sender: null,
                recipient: { peer: peerId, address: null },
                validUntil: null,
                body: kind === "item" ? null : { target: itemId },
                digest: "bd00f74cc22f8538a42a06a5227faf5e6b323622380dc9c61a3ad79ac72f3a4b",
              },
            },
          },
        },
      },
    },
  };
}

function websocketFrame(payload: Buffer, opcode = 2, finished = true): Buffer {
  const header = Buffer.alloc(payload.length < 126 ? 2 : 4);
  header[0] = opcode | (finished ? 128 : 0);
  header[1] = payload.length < 126 ? payload.length : 126;
  if (payload.length >= 126) header.writeUInt16BE(payload.length, 2);
  return Buffer.concat([header, payload]);
}

function openedAudit(): { audit: SessionAudit; connection: number } {
  const audit = new SessionAudit();
  const connection = audit.opened();
  audit.receive(connection, Buffer.from("HTTP/1.1 101 Switching Protocols\r\n\r\n"));
  return { audit, connection };
}

function send(audit: SessionAudit, connection: number, frame: unknown): void {
  audit.receive(connection, websocketFrame(Buffer.from(JSON.stringify(frame))));
}

test("a real crate Push observes the exact item id and counts the items stream", (): void => {
  const { audit, connection } = openedAudit();
  send(audit, connection, pushFrame());
  assert.partialDeepStrictEqual(audit.snapshot(), {
    pushes: { items: 1, controls: 0 },
    pushedItemIds: { items: [itemId], controls: [] },
    invalidConnections: 0,
  });
});

test("cancel Push ids count as controls and repeated deliveries remain visible", (): void => {
  const { audit, connection } = openedAudit();
  send(audit, connection, pushFrame("cancel", cancelId));
  send(audit, connection, pushFrame("item"));
  send(audit, connection, pushFrame("item"));
  assert.partialDeepStrictEqual(audit.snapshot(), {
    pushes: { items: 2, controls: 1 },
    pushedItemIds: { items: [itemId, itemId], controls: [cancelId] },
    invalidConnections: 0,
  });
});

test("fragmented Push bytes with an interleaved ping are observed only when complete", (): void => {
  const { audit, connection } = openedAudit();
  const bytes = Buffer.from(JSON.stringify(pushFrame()));
  const first = websocketFrame(bytes.subarray(0, 73), 2, false);
  const last = websocketFrame(bytes.subarray(73), 0);
  for (const byte of Buffer.concat([first, websocketFrame(Buffer.from("ping"), 9)]))
    audit.receive(connection, Buffer.from([byte]));
  const incomplete = audit.snapshot();
  for (const byte of last) audit.receive(connection, Buffer.from([byte]));
  assert.partialDeepStrictEqual(
    { incomplete, complete: audit.snapshot() },
    {
      incomplete: { pushes: { items: 0, controls: 0 }, pushedItemIds: { items: [], controls: [] } },
      complete: {
        pushes: { items: 1, controls: 0 },
        pushedItemIds: { items: [itemId], controls: [] },
        frames: { request: 1 },
        invalidConnections: 0,
      },
    },
  );
});

test("a mail-waiting hint or consumer Push lookalike is never a crate Push observation", (): void => {
  const { audit, connection } = openedAudit();
  send(audit, connection, {
    kind: "notice",
    value: {
      body: {
        kind: "consumer",
        value: {
          kind: { kind: "mailWaiting", value: { stream: "items" } },
        },
      },
    },
  });
  send(audit, connection, {
    kind: "request",
    value: {
      frame: frameId,
      body: { kind: "consumer", value: { kind: "push", value: { item: itemId } } },
    },
  });
  assert.partialDeepStrictEqual(audit.snapshot(), {
    pushes: { items: 0, controls: 0 },
    pushedItemIds: { items: [], controls: [] },
    invalidConnections: 0,
  });
});

for (const id of ["not-an-item-id", "01900000-0000-4000-8000-000000000001"]) {
  test(`a crate Push with an invalid item identity invalidates observation: ${id}`, (): void => {
    const { audit, connection } = openedAudit();
    send(audit, connection, pushFrame("item", id));
    assert.equal(audit.snapshot().invalidConnections, 1);
  });
}

test("a snapshot cannot mutate retained Push ids, including after a connection ends", (): void => {
  const { audit, connection } = openedAudit();
  send(audit, connection, pushFrame());
  const before = audit.snapshot();
  if (before.pushedItemIds !== undefined) Reflect.set(before.pushedItemIds.items, "0", cancelId);
  audit.ended(connection);
  assert.partialDeepStrictEqual(audit.snapshot(), {
    active: 0,
    pushes: { items: 1, controls: 0 },
    pushedItemIds: { items: [itemId], controls: [] },
  });
});

test("Session snapshots expose no retired mail-waiting counter", (): void => {
  const { audit, connection } = openedAudit();
  send(audit, connection, { kind: "notice", value: {} });
  assert.equal(Object.hasOwn(audit.snapshot(), "mailWaiting"), false);
});
