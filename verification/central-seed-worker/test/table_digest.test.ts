// The TypeScript digest port reproduces the Rust-recorded digests exactly.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";
import { parse } from "lossless-json";
import { seedValue } from "../src/seed_values.ts";
import { sha256Hex } from "../src/sha256_sync.ts";
import { serdeReal, tableDigest } from "../src/table_digest.ts";

export interface DigestCase {
  readonly name: string;
  readonly columns: string;
  readonly rows: readonly (readonly unknown[])[];
  readonly digest: string;
}
export async function digestCases(): Promise<DigestCase[]> {
  const text = await readFile(
    join(import.meta.dirname, "fixtures/table-digest-cases.json"),
    "utf8",
  );
  return (parse(text) as { cases: DigestCase[] }).cases;
}

test("the port matches the Rust TableDigestBuilder digest for every fixture case", async () => {
  for (const digestCase of await digestCases()) {
    const rows = digestCase.rows.map((row, index) =>
      row.map((value, column) => seedValue(value, `${digestCase.name}[${index}][${column}]`)),
    );
    assert.equal(tableDigest(rows), digestCase.digest, digestCase.name);
  }
});

// Each expected text is serde_json's own output, read from the Rust fixture test's probe run.
test("reals are written as serde_json writes them", () => {
  for (const [value, text] of [
    [1, "1.0"],
    [0.1, "0.1"],
    [-0, "-0.0"],
    [100, "100.0"],
    [1e16, "1e+16"],
    [1e15, "1000000000000000.0"],
    [1e21, "1e+21"],
    [1.5e-7, "1.5e-7"],
    [0.001, "0.001"],
    [0.00001, "0.00001"],
    [0.000001, "1e-6"],
    [2.5e-5, "0.000025"],
    [123456789012345680, "1.2345678901234568e+17"],
    [12345678901234567168, "1.2345678901234567e+19"],
    [5e-324, "5e-324"],
    [1.7976931348623157e308, "1.7976931348623157e+308"],
  ] as const) {
    assert.equal(serdeReal(value), text, String(value));
  }
});

test("the synchronous SHA-256 matches crypto.subtle at every padding boundary", async () => {
  for (const length of [0, 1, 55, 56, 63, 64, 65, 119, 120, 1000]) {
    const bytes = Uint8Array.from({ length }, (_, index) => (index * 31 + length) % 256);
    const expected = Buffer.from(await crypto.subtle.digest("SHA-256", bytes)).toString("hex");
    assert.equal(sha256Hex(bytes), expected, `length ${length}`);
  }
});
