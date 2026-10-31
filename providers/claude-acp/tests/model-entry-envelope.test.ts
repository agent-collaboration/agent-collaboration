import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFile } from "node:fs/promises";
import { createInterface } from "node:readline";
import { test } from "node:test";
import * as z from "zod/mini";
import { modelReasoningSchema } from "../src/model-metadata.ts";

await test("embedded lifecycle decorators admit fresh replacements and reject retired results", async (): Promise<void> => {
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
    { stdio: ["pipe", "pipe", "pipe"], signal: AbortSignal.timeout(5000) },
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
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGTERM");
  }
});
