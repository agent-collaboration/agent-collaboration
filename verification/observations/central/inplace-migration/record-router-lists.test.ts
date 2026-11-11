import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, realpath, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { createRecordedRouterReads } from "./record-router-lists.ts";
import { command } from "../../../machines/owned-process.ts";

test("captured public reads give the actual child190seconds and retain the ordinary30second default", async (context): Promise<void> => {
  const root = await realpath(await mkdtemp(join(tmpdir(), "2026-11-public-read-deadline-")));
  try {
    const evidenceDirectory = join(root, "evidence");
    const serviceDirectory = join(root, "agent-communication");
    await mkdir(evidenceDirectory, { mode: 0o700 });
    await mkdir(serviceDirectory, { mode: 0o700 });
    const timeouts: number[] = [];
    // Observe the real child-spawn option without scheduling or waiting on a native timer.
    context.mock.method(AbortSignal, "timeout", (milliseconds: number): AbortSignal => {
      timeouts.push(milliseconds);
      return new AbortController().signal;
    });
    const captured = createRecordedRouterReads({
      cliPath: process.execPath,
      serviceDirectory,
      evidenceDirectory,
    });
    const page = await captured.read([
      "-e",
      "process.stdout.write(JSON.stringify({ok:true,args:process.argv.slice(1)}))",
      "--",
    ]);
    assert.deepEqual(timeouts, [190000]);
    const bytes = await readFile(page.artifact);
    const reply: unknown = JSON.parse(bytes.toString());
    assert.deepEqual(reply, {
      ok: true,
      args: ["--service-directory", serviceDirectory, "--json"],
    });
    assert.equal(page.sha256, createHash("sha256").update(bytes).digest("hex"));
    assert.equal(captured.sources.length, 1);
    const ordinary = await command(process.execPath, ["-e", "process.stdout.write('ordinary')"]);
    assert.equal(ordinary.exit, 0);
    assert.deepEqual(timeouts, [190000, 30000]);
  } finally {
    await rm(root, { recursive: true });
  }
});
