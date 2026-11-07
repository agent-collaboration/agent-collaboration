import { checkedWorkerModules } from "./worker-modules.ts";
import { z } from "zod";
import {
  caseIdentities,
  requireCaseResults,
  recordSchema,
  replySchema,
  type WorkerReply,
} from "./case-contracts.ts";
import { Miniflare, Log, LogLevel, convertV4MiniflareOptions } from "miniflare";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { resolve, dirname } from "node:path";
import { randomUUIDv7 } from "node:crypto";
import assert from "node:assert/strict";

const fixture = z
  .object({ sender: z.unknown(), create: recordSchema, pieces: z.array(z.string()).min(2) })
  .parse(JSON.parse(await readFile(resolve(process.argv[2] ?? ""), "utf8")));
const evidence = resolve(process.argv[3] ?? "");
await mkdir(evidence, { recursive: true });
const buildDirectory = dirname(
  resolve(
    process.env.FLEET_WORKER_SHIM ?? "crates/fleet-project-copy/tests/workerd/build/shim.mjs",
  ),
);
const runtime = new Miniflare(
  convertV4MiniflareOptions({
    log: new Log(LogLevel.WARN),
    modules: checkedWorkerModules(buildDirectory),
    modulesRoot: buildDirectory,
    compatibilityDate: "2026-07-29",
    durableObjects: { PROBE: { className: "ProbeObject", useSQLite: true } },
    durableObjectsPersist: resolve(evidence, "state"),
    cf: false,
  }),
);
const observations = [];
async function call(name: string, body: string): Promise<WorkerReply> {
  const response = await runtime.dispatchFetch("http://copy.local/", {
    method: "POST",
    headers: { "X-Probe-Object": name },
    body,
    signal: AbortSignal.timeout(15000),
  });
  const text = await response.text();
  assert.equal(response.status, 200, text);
  return replySchema.parse(JSON.parse(text));
}
async function apply(name: string, piece: string, fault: string): Promise<WorkerReply> {
  // Piece JSON remains raw text: a JS parse would round its wide integers.
  return call(
    name,
    `{"action":"certaintyApply","sender":${JSON.stringify(fixture.sender)},"fault":${JSON.stringify(fault)},"piece":${piece}}`,
  );
}
async function counts(name: string): Promise<number[]> {
  const result = await call(
    name,
    JSON.stringify({
      action: "inspect",
      queries: [
        "SELECT CAST(count(*) AS TEXT) AS result FROM samples",
        "SELECT CAST(count(*) AS TEXT) AS result FROM _fleet_sync_receipts",
      ],
    }),
  );
  return z
    .array(z.array(z.string()))
    .parse(result.results)
    .map((rows) => Number(rows[0]));
}
try {
  await runtime.ready;
  for (const fault of [
    "rollbackAfterApply",
    "lostRollbackReply",
    "lostCommitReply",
    "unknownRollbackCode",
  ]) {
    const name = randomUUIDv7();
    await call(
      name,
      JSON.stringify({ action: "initialize", create: fixture.create, journal: true }),
    );
    assert.equal((await apply(name, z.string().parse(fixture.pieces[0]), "none")).kind, "applied");
    assert.deepEqual(await counts(name), [0, 1]);
    const refused = await apply(name, z.string().parse(fixture.pieces[1]), fault);
    const committed = fault === "lostCommitReply";
    assert.ok(refused.value?.reason);
    assert.equal(
      refused.kind,
      fault === "rollbackAfterApply" ? "copyUnavailable" : "outcomeUnknown",
    );
    assert.equal(
      refused.value.reason.kind,
      fault === "rollbackAfterApply" ? "damaged" : "storeBoundary",
    );
    assert.deepEqual(await counts(name), committed ? [1, 2] : [0, 1]);
    const repaired = await apply(name, z.string().parse(fixture.pieces[1]), "none");
    assert.equal(repaired.kind, committed ? "alreadyApplied" : "applied");
    assert.deepEqual(await counts(name), [1, 2]);
    assert.equal(
      (await apply(name, z.string().parse(fixture.pieces[1]), "none")).kind,
      "alreadyApplied",
    );
    assert.deepEqual(await counts(name), [1, 2]);
    observations.push({
      name: fault,
      kind: "passed",
      fault,
      refused,
      repaired,
      rows: 1,
      receipts: 2,
    });
  }
  requireCaseResults(observations, caseIdentities.certainty);
  await writeFile(resolve(evidence, "results.json"), JSON.stringify(observations, null, 2));
  console.log(JSON.stringify({ passed: observations.length, failed: 0, observations }));
} finally {
  await runtime.dispose();
}
