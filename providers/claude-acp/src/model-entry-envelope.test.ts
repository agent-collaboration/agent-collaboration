import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, readdir, rmdir, unlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { test } from "node:test";
import * as z from "zod/mini";
import { modelReasoningSchema } from "./model-metadata.ts";

await test("embedded lifecycle decorators admit fresh replacements and reject retired results", async (): Promise<void> => {
  const temporaryRoot = await mkdtemp(join(tmpdir(), "model-entry-test-"));
  const source = await readFile(
    new URL("../../../crates/acp-provider-adapters/src/claude_metadata.mjs", import.meta.url),
    "utf8",
  );
  const child = spawn(
    process.execPath,
    [
      "--input-type=module",
      "--eval",
      source,
      "--",
      new URL("./fixtures/public-model-library.ts", import.meta.url).href,
      new URL("./fixtures/public-sdk.ts", import.meta.url).href,
    ],
    {
      env: {
        ...process.env,
        TMPDIR: temporaryRoot,
        ANTHROPIC_BASE_URL: "http://127.0.0.1:31415/anthropic",
        ANTHROPIC_AUTH_TOKEN: "fixture-host-token",
      },
      stdio: ["pipe", "pipe", "pipe"],
      signal: AbortSignal.timeout(5000),
    },
  );
  const exit = once(child, "exit");
  const rowSchema = z.object({
    method: z.enum(["loadSession", "resumeSession"]),
    retired: z.boolean(),
    result: z.looseObject({
      _meta: z.looseObject({
        codexRouter: z.looseObject({ modelReasoning: modelReasoningSchema }),
      }),
    }),
  });
  let stderr = "";
  child.stderr.setEncoding("utf8").on("data", (value: string): void => {
    stderr += value;
  });
  const rows: z.infer<typeof rowSchema>[] = [];
  try {
    for await (const line of createInterface({ input: child.stdout })) {
      const decoded: unknown = JSON.parse(line);
      const row = rowSchema.safeParse(decoded);
      if (row.success) {
        rows.push(row.data);
        continue;
      }
      const { calls } = z.object({ calls: z.array(z.string()) }).parse(decoded);
      assert.deepEqual(calls, ["loadSession", "resumeSession", "loadSession", "resumeSession"]);
      break;
    }
    assert.equal(rows.length, 4);
    for (const row of rows) {
      const facts = row.result._meta.codexRouter.modelReasoning;
      assert.equal(
        facts.kind,
        row.retired ? "unknown" : "available",
        `${row.method} retired=${row.retired}`,
      );
      if (facts.kind === "available")
        assert.deepEqual(facts.models, [
          {
            modelId: "sonnet",
            kind: "select",
            configId: "effort",
            defaultValue: "default",
            choices: [
              { value: "default", label: "Default" },
              { value: "low", label: "Low" },
              { value: "high", label: "High" },
            ],
          },
        ]);
    }
    child.stdin.end();
    const [code, signal] = await exit;
    assert.equal(code, 0, stderr);
    assert.equal(signal, null);
  } finally {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGTERM");
      await exit;
    }
    for (const entry of await readdir(temporaryRoot)) {
      const directory = join(temporaryRoot, entry);
      for (const file of await readdir(directory)) await unlink(join(directory, file));
      await rmdir(directory);
    }
    await rmdir(temporaryRoot);
  }
});
