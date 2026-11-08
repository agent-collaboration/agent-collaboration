import { test } from "node:test";
import assert from "node:assert/strict";
import { WindowByteAudit } from "./window-byte-audit.ts";
import { encode } from "../../../exact-json.ts";
import { replayWindowBytes } from "./window-readback.ts";
import { execFileSync } from "node:child_process";
import { randomUUIDv7 } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { writeFile, unlink } from "node:fs/promises";

function identity(index: number): string {
  return `01900000-0000-7000-8000-${index.toString(16).padStart(12, "0")}`;
}
function frame(value: unknown, masked = false, opcode = 2, finished = true): Buffer {
  const payload = Buffer.isBuffer(value) ? Buffer.from(value) : Buffer.from(encode(value));
  const header = Buffer.alloc(payload.length < 126 ? 2 : 4);
  header[0] = opcode | (finished ? 128 : 0);
  header[1] = (payload.length < 126 ? payload.length : 126) | (masked ? 128 : 0);
  if (payload.length >= 126) header.writeUInt16BE(payload.length, 2);
  if (!masked) return Buffer.concat([header, payload]);
  const mask = Buffer.from([7, 11, 13, 17]);
  for (let index = 0; index < payload.length; index += 1)
    payload[index] = payload.readUInt8(index) ^ mask.readUInt8(index % 4);
  return Buffer.concat([header, mask, payload]);
}
function opened(): WindowByteAudit {
  const audit = new WindowByteAudit();
  audit.opened("a");
  audit.client("a", Buffer.from("GET /v1/session HTTP/1.1\r\nHost: fixture\r\n\r\n"));
  audit.server("a", Buffer.from("HTTP/1.1 101 Switching Protocols\r\n\r\n"));
  audit.client("a", frame({ kind: "hello", value: {} }, true));
  audit.server("a", frame({ kind: "welcome", value: {} }));
  return audit;
}
function batch(size: number, start = 1): unknown {
  return {
    kind: "requestBatch",
    value: Array.from({ length: size }, (_, index) => ({
      sendId: identity(start + index),
      body: {
        kind: "crate",
        value: {
          kind: "push",
          value: { item: { kind: "item", value: { id: identity(start + index) } } },
        },
      },
    })),
  };
}

test("literal original batch bytes and masked per-item answers determine the actual window", (): void => {
  const audit = opened();
  audit.server("a", frame(batch(32)));
  audit.client(
    "a",
    frame(
      {
        kind: "answerBatch",
        value: [
          {
            sendId: identity(1),
            body: {
              kind: "crate",
              value: { kind: "pushed", value: { kind: "accepted", value: {} } },
            },
          },
        ],
      },
      true,
    ),
  );
  audit.server("a", frame(batch(1, 33)));
  audit.requireWindow();
  assert.equal(audit.snapshot().window.peak, 32);
  assert.equal(audit.snapshot().wire[0]?.server.pushes.items, 33);
  assert.equal(audit.snapshot().window.connections[0]?.current, 32);
});

test("two legal encoded batches containing 33 unanswered items fail the deciding window assertion", (): void => {
  const audit = opened();
  audit.server("a", frame(batch(32)));
  audit.server("a", frame(batch(1, 33)));
  assert.throws((): void => audit.requireWindow(), /window exceeds 32: 33/);
});

test("a partial fragmented batch cannot qualify while live or after disconnect", (): void => {
  const audit = opened();
  const bytes = Buffer.from(encode(batch(32)));
  audit.server("a", frame(bytes.subarray(0, 70), false, 2, false));
  assert.throws((): void => audit.requireWindow(), /incomplete/);
  audit.ended("a");
  assert.throws((): void => audit.requireWindow(), /invalid/);
});

test("only a complete fragmented message after interleaved control ping contributes items", (): void => {
  const audit = opened();
  const bytes = Buffer.from(encode(batch(2)));
  audit.server("a", frame(bytes.subarray(0, 70), false, 2, false));
  audit.server("a", frame(Buffer.from("probe"), false, 9));
  assert.equal(audit.snapshot().window.sent, 0);
  audit.server("a", frame(bytes.subarray(70), false, 0));
  audit.requireWindow();
  assert.equal(audit.snapshot().window.sent, 2);
});

test("invalid masked bytes, duplicate batch send ids and unknown frames are never ignored", (): void => {
  for (const wrong of [
    frame(batch(1), true),
    frame({
      kind: "requestBatch",
      value: [
        { sendId: identity(1), body: {} },
        { sendId: identity(1), body: {} },
      ],
    }),
    frame({ kind: "unknownActualFrame", value: {} }),
  ]) {
    const audit = opened();
    audit.server("a", wrong);
    assert.throws((): void => audit.requireWindow(), /invalid/);
  }
});

test("the standalone reader consumes its actual private JSONL byte format and never emits runtime credit", async (): Promise<void> => {
  const recordBytes = (kind: "client" | "server", bytes: Buffer): unknown => ({
    kind,
    connection: "a",
    bytes: bytes.toString("base64"),
  });
  const rows = [
    { kind: "opened", connection: "a" },
    recordBytes("client", Buffer.from("GET /v1/session HTTP/1.1\r\nHost: fixture\r\n\r\n")),
    recordBytes("server", Buffer.from("HTTP/1.1 101 Switching Protocols\r\n\r\n")),
    recordBytes("client", frame({ kind: "hello", value: {} }, true)),
    recordBytes("server", frame({ kind: "welcome", value: {} })),
    recordBytes("server", frame(batch(1))),
    { kind: "ended", connection: "a" },
  ];
  const contents = `${rows.map(encode).join("\n")}\n`;
  assert.equal(replayWindowBytes(contents).window.peak, 1);
  assert.throws((): void => {
    replayWindowBytes(`${contents}${encode({ kind: "server", connection: "a", bytes: "??" })}\n`);
  }, /noncanonical/);
  const path = join(tmpdir(), `d4-window-${randomUUIDv7()}.jsonl`);
  await writeFile(path, contents, { mode: 0o600, flag: "wx" });
  try {
    const result: unknown = JSON.parse(
      execFileSync(
        process.execPath,
        [new URL("./window-readback.ts", import.meta.url).pathname, path],
        { encoding: "utf8" },
      ),
    );
    assert.partialDeepStrictEqual(result, {
      scope: "retained-byte measurement only",
      runtimeQualified: false,
    });
  } finally {
    await unlink(path);
  }
});
