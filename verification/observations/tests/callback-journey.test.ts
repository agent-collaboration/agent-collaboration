import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { decodeEvidenceJson } from "../../evidence-json.ts";
import { driveCallbacks } from "../interaction-journey.ts";
import { runCallbackFixture } from "./callback-journey-fixture.ts";

test(
  "complete callback journey matches the original over HTTP and real SQLite readback",
  { timeout: 10_000 },
  async (context): Promise<void> => {
    const stdout: unknown[] = [];
    context.mock.method(console, "log", (text: string): void => {
      stdout.push(decodeEvidenceJson(text));
    });
    const result = await runCallbackFixture(
      async (plan): Promise<{ readonly exit: number; readonly stdout: readonly unknown[] }> => {
        await driveCallbacks(plan);
        return { exit: 0, stdout };
      },
    );
    assert.deepEqual(
      result,
      decodeEvidenceJson(readFileSync(new URL("baselines/callback-journey.bin", import.meta.url))),
    );
  },
);
