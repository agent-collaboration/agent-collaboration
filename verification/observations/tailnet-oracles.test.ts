import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { z } from "zod";
import { decodeEvidenceJson } from "../evidence-json.ts";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import { boardMarkerSchema, httpsEndpointSchema, requireMarker } from "./tailnet-plans.ts";
import { curlHttpReply, curlPostArguments } from "./tailnet-transport.ts";
import { readStoredMarker } from "./tailnet-board-readback.ts";

const numericSchema = z.number().or(z.bigint()).transform(Number);
const binarySchema = z.union([
  z.object({ base64: z.string() }),
  z.object({ repeatByte: numericSchema, count: numericSchema, suffixBase64: z.string() }),
]);
function bytes(value: unknown): Buffer {
  const packed = binarySchema.parse(value);
  return "base64" in packed
    ? Buffer.from(packed.base64, "base64")
    : Buffer.concat([
        Buffer.alloc(packed.count, packed.repeatByte),
        Buffer.from(packed.suffixBase64, "base64"),
      ]);
}
const schema = z.object({
  function: z.enum([
    "curl_post_arguments",
    "curl_http_reply",
    "require_marker",
    "read_stored_marker",
  ]),
  args: z.array(z.unknown()),
  databaseFile: z.string().optional(),
  expected: z.union([z.object({ result: z.unknown() }), z.object({ refused: z.literal(true) })]),
});
const directory = new URL("./baselines/", import.meta.url);
for (const filename of readdirSync(directory)
  .filter((name) => /^tailnet-oracle-\d+\.json$/.test(name))
  .sort()) {
  const entry = schema.parse(decodeEvidenceJson(readFileSync(new URL(filename, directory))));
  test(`tailnet curl/marker/store oracle matches original ${entry.function} ${filename}`, (): void => {
    const root = createFixtureDirectory("tailnet-oracle");
    const database = join(root, "board.sqlite");
    if (entry.databaseFile !== undefined)
      copyFileSync(new URL(entry.databaseFile, directory), database);
    const before = existsSync(database)
      ? createHash("sha256").update(readFileSync(database)).digest("hex")
      : null;
    const invoke = (): unknown => {
      switch (entry.function) {
        case "curl_post_arguments":
          return curlPostArguments(
            httpsEndpointSchema.parse(entry.args[0]),
            z.record(z.string(), z.string()).parse(entry.args[1]),
          );
        case "curl_http_reply": {
          const completed = z
            .object({ exit: numericSchema, stdout: z.unknown(), stderr: z.unknown() })
            .parse(entry.args[0]);
          const reply = curlHttpReply({
            exit: completed.exit,
            stdout: bytes(completed.stdout),
            stderr: bytes(completed.stderr),
          });
          return { status: BigInt(reply.status), body: reply.body.toString() };
        }
        case "require_marker":
          return (
            requireMarker(entry.args[0], boardMarkerSchema.parse(entry.args[1]), entry.args[2]) ??
            null
          );
        case "read_stored_marker":
          return (
            readStoredMarker(database, boardMarkerSchema.parse(entry.args[1]), entry.args[2]) ??
            null
          );
      }
    };
    try {
      if ("result" in entry.expected) assert.deepEqual(invoke(), entry.expected.result);
      else assert.throws(invoke);
      if (before !== null)
        assert.equal(createHash("sha256").update(readFileSync(database)).digest("hex"), before);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
