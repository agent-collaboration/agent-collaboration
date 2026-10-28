import assert from "node:assert/strict";
import { closeSync, openSync, readSync, writeSync } from "node:fs";

export function releasePipeBarrier(readyPath: string, goPath: string): void {
  const ready = openSync(readyPath, "w");
  try {
    writeSync(ready, Buffer.from("R"));
  } finally {
    closeSync(ready);
  }
  const gate = openSync(goPath, "r");
  try {
    const byte = Buffer.alloc(1);
    assert.equal(readSync(gate, byte), 1);
    assert.equal(byte.toString(), "G");
  } finally {
    closeSync(gate);
  }
}
