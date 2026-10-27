import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { test } from "node:test";
import { z } from "zod";
import { decode, encode } from "../../exact-json.ts";
import {
  codexMetadataThreadSchema,
  metadataObservationResultSchema,
  providerObservationSchema,
} from "../acp-metadata-models.ts";
import { checkMetadataObservation, terminalText } from "../acp-metadata-oracle.ts";

const schema = z.object({
  name: z.string(),
  observation: z.unknown(),
  views: z.array(z.unknown()),
  terminal: z.string(),
  resumedTerminal: z.string(),
  expectedReply: z.string(),
  requireUsage: z.boolean(),
  expected: z.union([
    z.object({ result: metadataObservationResultSchema }),
    z.object({ error: z.string() }),
  ]),
});
const directory = new URL("baselines/", import.meta.url);
for (const filename of readdirSync(directory)
  .filter((name) => name.startsWith("metadata-"))
  .sort()) {
  const entry = schema.parse(JSON.parse(readFileSync(new URL(filename, directory), "utf8")));
  test(`metadata oracle matches original ${entry.name} (${filename})`, (): void => {
    const input = {
      observation: providerObservationSchema.parse(decode(encode(entry.observation))),
      views: entry.views.map((view) => codexMetadataThreadSchema.parse(view)),
      terminal: entry.terminal,
      resumedTerminal: entry.resumedTerminal,
      expectedReply: entry.expectedReply,
      requireUsage: entry.requireUsage,
    };
    let actual;
    try {
      actual = { result: checkMetadataObservation(input) };
    } catch (error) {
      actual = { error: error instanceof Error ? error.message : String(error) };
    }
    assert.deepEqual(actual, entry.expected);
  });
}

test("terminal control decoding preserves the original CRLF, CSI and OSC normalization", (): void => {
  assert.equal(
    terminalText(Buffer.from("0d0a1b5b326de280a2201b5b32326d4143505f4f4b0d0a", "hex")),
    "\n• ACP_OK\n",
  );
  assert.equal(terminalText(Buffer.from("1b5b33326d4143505f4f4b1b5b306d", "hex")), "ACP_OK");
  assert.equal(terminalText(Buffer.from("1b5d303b7469746c65074143505f4f4b", "hex")), "ACP_OK");
  assert.equal(terminalText(Buffer.from("1b5d303b7469746c651b5c4143505f4f4b", "hex")), "ACP_OK");
});
