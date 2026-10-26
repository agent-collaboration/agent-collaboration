import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { z } from "zod";
import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import { decodeEvidenceJson } from "../../evidence-json.ts";

const baselineSchema = z.object({
  original: z.string(),
  originalSHA256: z.string(),
  target: z.string(),
  arguments: z.array(z.string()),
  stdin: z.string(),
  expected: z.object({
    exit: z.number().or(z.bigint()).transform(Number),
    stdout: z.union([
      z.string(),
      z.object({ sha256: z.string(), bytes: z.number().or(z.bigint()).transform(Number) }),
    ]),
    stderr: z.string(),
    receipt: z.string().nullable(),
  }),
});
const directory = new URL("baselines/", import.meta.url);
for (const name of readdirSync(directory)
  .filter((name) => /^embedded-acp-\d+\.json$/.test(name))
  .sort()) {
  const fixture = baselineSchema.parse(decodeEvidenceJson(readFileSync(new URL(name, directory))));
  test(`embedded ACP port matches original wire and receipt bytes: ${name} ${fixture.target}`, (): void => {
    const root = createFixtureDirectory("embedded-acp");
    const receipt = join(root, "receipt.json");
    try {
      const result = spawnSync(
        process.execPath,
        [
          new URL(`../${fixture.target}`, import.meta.url).pathname,
          ...fixture.arguments.map((argument) => (argument === "<RECEIPT>" ? receipt : argument)),
        ],
        { input: fixture.stdin, encoding: "utf8", timeout: 5000, maxBuffer: 4 * 1024 * 1024 },
      );
      if (result.error !== undefined) throw result.error;
      assert.deepEqual(
        {
          exit: result.status,
          stdout:
            typeof fixture.expected.stdout === "string"
              ? result.stdout
              : {
                  sha256: createHash("sha256").update(result.stdout).digest("hex"),
                  bytes: Buffer.byteLength(result.stdout),
                },
          stderr: result.stderr,
          receipt: existsSync(receipt) ? readFileSync(receipt, "utf8") : null,
        },
        fixture.expected,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
