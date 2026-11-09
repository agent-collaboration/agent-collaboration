import { test } from "node:test";
import assert from "node:assert/strict";
import { D4WireCapture } from "./wire-capture.ts";
import { decodeWindowTrace } from "./window-readback.ts";

test("wire capture freezes exact bytes and rejects an empty or oversized transcript permanently", (): void => {
  const capture = new D4WireCapture();
  assert.throws((): void => {
    capture.transcript();
  }, /no original/);
  const bytes = Buffer.from([0, 255, 17, 128]);
  capture.observe({ kind: "opened", connection: "physical-a" });
  capture.observe({ kind: "server", connection: "physical-a", bytes: bytes.toString("base64") });
  bytes.fill(0);
  const record = decodeWindowTrace(capture.transcript())[1];
  assert.ok(record?.kind === "server");
  assert.deepEqual(Buffer.from(record.bytes, "base64"), Buffer.from([0, 255, 17, 128]));
  assert.throws((): void => {
    capture.observe({
      kind: "server",
      connection: "physical-a",
      bytes: "A".repeat(32 * 1024 * 1024),
    });
  }, /exceeds/);
  assert.throws((): void => {
    capture.transcript();
  }, /invalid/);
});
