import { checkedWorkerModules } from "./worker-modules.ts";
import { z } from "zod";
import {
  caseIdentities,
  requireCaseResults,
  recordSchema,
  replySchema,
  type WorkerReply,
} from "./case-contracts.ts";
import {
  Miniflare,
  Log,
  LogLevel,
  convertV4MiniflareOptions,
  type V4MiniflareOptions,
} from "miniflare";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { resolve, dirname } from "node:path";
import { randomUUIDv7 } from "node:crypto";
import assert from "node:assert/strict";

const fixture = resolve(process.argv[2] ?? "");
const checkpointSchema = z.object({
  name: z.string(),
  queries: z.array(z.string()),
  expected: z.array(z.unknown()),
});
const manifest = z
  .object({
    cases: z.array(
      z.object({
        name: z.string(),
        create: recordSchema,
        sender: z.unknown(),
        pieces: z.array(z.string()),
        checkpoints: z.record(z.string(), checkpointSchema),
        targetProduced: z.boolean(),
        writerStatus: z.unknown(),
      }),
    ),
    snapshots: z
      .array(
        z.object({
          name: z.string(),
          install: recordSchema,
          source: recordSchema,
          pages: z.array(z.object({ path: z.string(), index: z.number(), cursor: z.unknown() })),
          manifest: recordSchema,
          checkpoint: checkpointSchema,
        }),
      )
      .optional(),
  })
  .parse(JSON.parse(await readFile(fixture, "utf8")));
requireCaseResults(
  manifest.cases.map((item) => ({
    name: item.name,
    kind: item.targetProduced ? "passed" : "prefixOnly",
  })),
  caseIdentities.rename,
);
const root = resolve(import.meta.dirname);
const evidence = resolve(root, "evidence", process.argv[3] ?? "matrix-01");
await mkdir(evidence, { recursive: true });
const buildDirectory = dirname(
  resolve(
    process.env.FLEET_WORKER_SHIM ?? "crates/fleet-project-copy/tests/workerd/build/shim.mjs",
  ),
);
const options = {
  host: "127.0.0.1",
  port: 0,
  log: new Log(LogLevel.WARN),
  modules: checkedWorkerModules(buildDirectory),
  modulesRoot: buildDirectory,
  compatibilityDate: "2026-07-29",
  durableObjects: { PROBE: { className: "ProbeObject", useSQLite: true } },
  durableObjectsPersist: resolve(evidence, "state"),
  cf: false,
} satisfies V4MiniflareOptions;
let runtime = new Miniflare(convertV4MiniflareOptions(options));
const results = [];
async function call(name: string, body: string): Promise<WorkerReply> {
  const response = await runtime.dispatchFetch("http://normal.local/", {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Probe-Object": name },
    body,
    signal: AbortSignal.timeout(30000),
  });
  const text = await response.text();
  if (response.status !== 200) throw Error(`HTTP${response.status}: ${text}`);
  return replySchema.parse(JSON.parse(text));
}
function normalize(rows: string[][]): unknown[][] {
  return rows.map((values) => values.map((value) => JSON.parse(value)));
}
async function inspect(
  name: string,
  checkpoint: z.infer<typeof checkpointSchema>,
): Promise<unknown> {
  const result = await call(
    name,
    JSON.stringify({ action: "inspect", queries: checkpoint.queries }),
  );
  assert.deepEqual(
    normalize(z.array(z.array(z.string())).parse(result.results)),
    checkpoint.expected,
  );
  return { name: checkpoint.name, matched: true, results: result.results };
}
async function inspectIfPresent(
  name: string,
  checkpoint: z.infer<typeof checkpointSchema> | undefined,
  checkpoints: unknown[],
): Promise<void> {
  if (checkpoint) checkpoints.push(await inspect(name, checkpoint));
}
try {
  await runtime.ready;
  for (const item of manifest.cases) {
    const name = "normal-" + item.name + "-" + randomUUIDv7();
    const log = [];
    const checkpoints: unknown[] = [];
    try {
      await call(name, JSON.stringify({ action: "initialize", create: item.create }));
      if (item.checkpoints?.["0"]) checkpoints.push(await inspect(name, item.checkpoints["0"]));
      for (const [index, path] of item.pieces.entries()) {
        const raw = await readFile(resolve(dirname(fixture), path), "utf8");
        const answer = await call(
          name,
          '{"action":"apply","piece":' + raw + ',"sender":' + JSON.stringify(item.sender) + "}",
        );
        log.push(answer);
        assert.ok(["applied", "staged", "duplicate"].includes(answer.kind), JSON.stringify(answer));
        const checkpoint = item.checkpoints[String(index + 1)];
        await inspectIfPresent(name, checkpoint, checkpoints);
      }
      results.push({
        name: item.name,
        kind: item.targetProduced ? "passed" : "prefixOnly",
        writer: item.writerStatus,
        log,
        checkpoints,
      });
    } catch (error) {
      results.push({ name: item.name, kind: "failed", error: String(error), log, checkpoints });
    }
  }
  // A fresh object is populated by the public snapshot rules, never by manual INSERT fixtures.
  for (const item of manifest.snapshots ?? []) {
    const name = "snapshot-" + item.name + "-" + randomUUIDv7();
    const log = [];
    try {
      log.push(
        await call(
          name,
          JSON.stringify({ action: "snapshotPrepare", install: item.install, source: item.source }),
        ),
      );
      for (const page of item.pages) {
        const raw = await readFile(resolve(dirname(fixture), page.path), "utf8");
        log.push(
          await call(
            name,
            '{"action":"snapshotPage","install":' +
              JSON.stringify(item.install) +
              ',"index":' +
              page.index +
              ',"cursor":' +
              JSON.stringify(page.cursor) +
              ',"page":' +
              raw +
              "}",
          ),
        );
      }
      log.push(
        await call(
          name,
          JSON.stringify({
            action: "snapshotFinish",
            install: item.install,
            manifest: item.manifest,
          }),
        ),
      );
      const checkpoint = await inspect(name, item.checkpoint);
      results.push({ name: item.name, kind: "snapshotPassed", checkpoint, log });
    } catch (error) {
      results.push({ name: item.name, kind: "snapshotFailed", error: String(error), log });
    }
  }
  await writeFile(resolve(evidence, "results.json"), JSON.stringify(results, null, 2));
  requireCaseResults(
    results.filter((row) => caseIdentities.rename.includes(row.name)),
    caseIdentities.rename,
  );
  if (manifest.snapshots?.length)
    requireCaseResults(
      results.filter((row) => !caseIdentities.rename.includes(row.name)),
      manifest.snapshots.map((row) => row.name),
      "snapshotPassed",
    );

  console.log(
    JSON.stringify(
      results.map(({ name, kind, error, checkpoints }) => ({
        name,
        kind,
        error,
        checkpoints: checkpoints?.length,
      })),
    ),
  );
  if (results.some((result) => ["failed", "snapshotFailed"].includes(result.kind)))
    process.exitCode = 1;
} finally {
  await runtime.dispose();
}
