import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFile } from "node:fs/promises";
import { createInterface } from "node:readline";
import { test } from "node:test";
import { notificationSchema } from "../src/skill-metadata.ts";

for (const ending of ["eof", "SIGINT", "SIGTERM"]) {
  await test(`embedded entry preserves policy, outgoing metadata and one disposal on ${ending}`, async (): Promise<void> => {
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
        new URL("./fixtures/public-library.ts", import.meta.url).href,
        new URL("./fixtures/public-sdk.ts", import.meta.url).href,
      ],
      {
        env: {
          ...process.env,
          ANTHROPIC_BASE_URL: "http://127.0.0.1:31415/anthropic",
          ANTHROPIC_AUTH_TOKEN: "fixture-host-token",
        },
        stdio: ["pipe", "pipe", "pipe"],
        signal: AbortSignal.timeout(5000),
      },
    );
    let stderr = "";
    child.stderr.setEncoding("utf8").on("data", (text: string): void => {
      stderr += text;
    });
    const exit = once(child, "exit");
    let annotated = false;
    try {
      const lines = createInterface({ input: child.stdout });
      for await (const line of lines) {
        const notification = notificationSchema.parse(JSON.parse(line));
        if (notification.update._meta?.codexRouter !== undefined) {
          annotated = true;
          break;
        }
      }
      assert.equal(annotated, true);
      if (ending === "eof") child.stdin.end();
      else child.kill(ending === "SIGINT" ? "SIGINT" : "SIGTERM");
      const [code, signal] = await exit;
      assert.equal(code, 0, stderr);
      assert.equal(signal, null);
      assert.equal(stderr.split("fixture-disposed").length - 1, 1);
    } finally {
      if (child.exitCode === null && child.signalCode === null) child.kill("SIGTERM");
    }
  });
}
