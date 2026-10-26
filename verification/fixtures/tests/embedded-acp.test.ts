import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
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
  initialFiles: z.record(z.string(), z.string()).optional(),
  pidFiles: z.array(z.string()).optional(),
  expected: z.object({
    exit: z.number().or(z.bigint()).transform(Number),
    stdout: z.union([
      z.string(),
      z.object({ sha256: z.string(), bytes: z.number().or(z.bigint()).transform(Number) }),
    ]),
    stderr: z.string(),
    receipt: z.string().nullable(),
    files: z.record(z.string(), z.string()).optional(),
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
      for (const [name, content] of Object.entries(fixture.initialFiles ?? {}))
        writeFileSync(join(root, name), content);
      const result = spawnSync(
        process.execPath,
        [
          new URL(`../${fixture.target}`, import.meta.url).pathname,
          ...fixture.arguments.map((argument) =>
            argument === "<RECEIPT>" ? receipt : argument.replaceAll("<ROOT>", root),
          ),
        ],
        {
          input: fixture.stdin.replaceAll("<ROOT>", root),
          encoding: "utf8",
          timeout: 5000,
          maxBuffer:
            typeof fixture.expected.stdout === "string"
              ? 4 * 1024 * 1024
              : Math.max(4 * 1024 * 1024, fixture.expected.stdout.bytes + 1024),
        },
      );
      if (result.error !== undefined) throw result.error;
      const readRetainedFile = (filename: string): string => {
        const content = readFileSync(join(root, filename), "utf8");
        if (fixture.pidFiles?.includes(filename)) {
          assert.equal(content, String(result.pid));
          return "<OWN_PID>";
        }
        return content;
      };
      assert.deepEqual(
        {
          exit: result.status,
          stdout:
            typeof fixture.expected.stdout === "string"
              ? result.stdout.replaceAll(root, "<ROOT>")
              : {
                  sha256: createHash("sha256").update(result.stdout).digest("hex"),
                  bytes: Buffer.byteLength(result.stdout),
                },
          stderr: result.stderr,
          receipt: existsSync(receipt) ? readRetainedFile("receipt.json") : null,
          ...(fixture.expected.files === undefined
            ? {}
            : {
                files: Object.fromEntries(
                  readdirSync(root).map((filename) => [filename, readRetainedFile(filename)]),
                ),
              }),
        },
        fixture.expected,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
