import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { z } from "zod";
import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import { decode } from "../../exact-json.ts";
import { evidenceJson } from "../../evidence-json.ts";
import {
  callbackPrompt,
  compareReopen,
  decodeRpcBody,
  questionResponseFor,
  requireAdvertisedSettings,
  requireCallbackCapture,
  readSnapshot,
} from "../interaction-readback.ts";

const schema = z.object({
  function: z.enum([
    "require_advertised_settings",
    "callback_prompt",
    "decode_rpc_body",
    "question_response_for",
    "require_callback_capture",
    "read_snapshot",
    "compare_reopen",
  ]),
  name: z.string(),
  args: z.array(z.unknown()),
  databasePresent: z.boolean().optional(),
  databaseFile: z.string().optional(),
  expected: z.union([
    z.object({ result: z.unknown() }),
    z.object({ error: z.string() }),
    z.object({ validationError: z.literal(true) }),
    z.object({ missingDatabase: z.literal(true) }),
  ]),
});
const directory = new URL("baselines/", import.meta.url);
for (const filename of readdirSync(directory)
  .filter((name) => name.startsWith("callback-") && name.endsWith(".json"))
  .sort()) {
  const entry = schema.parse(decode(readFileSync(new URL(filename, directory), "utf8")));
  test(`callback apparatus matches original ${entry.name} (${filename})`, (): void => {
    const root = createFixtureDirectory("callback-oracle");
    const database = join(root, "input.sqlite");
    if (entry.databaseFile !== undefined)
      copyFileSync(new URL(entry.databaseFile, directory), database);
    const before = existsSync(database)
      ? createHash("sha256").update(readFileSync(database)).digest("hex")
      : null;
    function invoke(): unknown {
      const args = entry.args;
      switch (entry.function) {
        case "require_advertised_settings":
          return requireAdvertisedSettings(
            args[0],
            z.record(z.string(), z.string()).parse(args[1]),
          );
        case "callback_prompt":
          return callbackPrompt(
            z.record(z.string(), z.string()).parse(args[0]),
            z.string().parse(args[1]),
          );
        case "decode_rpc_body":
          return decodeRpcBody(z.string().parse(args[0]));
        case "question_response_for":
          return questionResponseFor(args[0]);
        case "require_callback_capture":
          return requireCallbackCapture(args[0], args[1], z.array(z.string()).parse(args[2]));
        case "compare_reopen":
          return compareReopen(args[0], args[1]);
        case "read_snapshot": {
          const result = readSnapshot(database, z.array(z.string()).parse(args[1]));
          return { ...result, database: "<database>" };
        }
      }
    }
    try {
      if ("result" in entry.expected)
        assert.equal(evidenceJson(invoke() ?? null), evidenceJson(entry.expected.result));
      else if ("validationError" in entry.expected) assert.throws(invoke, z.ZodError);
      else if ("missingDatabase" in entry.expected) {
        assert.throws(invoke);
        assert.equal(existsSync(database), false);
      } else {
        assert.throws(
          invoke,
          (error: unknown): boolean =>
            error instanceof Error &&
            "error" in entry.expected &&
            error.message === entry.expected.error,
        );
      }
      if (before !== null)
        assert.equal(createHash("sha256").update(readFileSync(database)).digest("hex"), before);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
