import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import assert from "node:assert/strict";
import { cpSync, readFileSync, rmSync } from "node:fs";
import { createInterface } from "node:readline";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { test } from "node:test";
import { z } from "zod";
import { reportHtml } from "../harness-report.ts";

const fixture = new URL("baselines/harness-report/", import.meta.url);

test("harness report HTML bytes match the original including escaping and wide integer evidence", (): void => {
  assert.deepEqual(reportHtml(fixture.pathname), readFileSync(new URL("expected.bin", fixture)));
});

test(
  "harness report CLI serves its captured evidence on an owned loopback listener",
  { timeout: 5000 },
  async (context): Promise<void> => {
    const root = createFixtureDirectory("harness-report");
    cpSync(fixture, root, { recursive: true });
    const child = spawn(
      process.execPath,
      [new URL("../harness-report.ts", import.meta.url).pathname, root],
      { stdio: ["ignore", "pipe", "pipe"] },
    );
    context.after(async (): Promise<void> => {
      if (child.exitCode === null && child.signalCode === null) {
        child.kill();
        await once(child, "close");
      }
      rmSync(root, { recursive: true, force: true });
    });
    const lines = createInterface({ input: child.stdout });
    context.after(() => {
      lines.close();
    });
    const [line] = await once(lines, "line");
    const ready = z
      .object({ url: z.url(), label: z.literal("harness UI, not native app") })
      .parse(JSON.parse(z.string().parse(line)));
    assert.equal(new URL(ready.url).hostname, "127.0.0.1");
    const response = await fetch(ready.url);
    assert.equal(response.status, 200);
    assert.deepEqual(
      Buffer.from(await response.arrayBuffer()),
      readFileSync(new URL("expected.bin", fixture)),
    );
    assert.equal((await fetch(`${ready.url}missing`)).status, 404);
  },
);
