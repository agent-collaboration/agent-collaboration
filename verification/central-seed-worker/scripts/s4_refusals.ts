// S4 on staging (T11), one phase per call, against the S4 refusal allocation.
// The deployed Worker alternates: the seed Worker (deploy_seed_worker.ts, then
// --s4-redeploy) for baseline/apply/undo, the migration Worker
// (deploy_worker.ts migration) for migrate. Every answer and image is kept in
// <receipts>, mode 0600; nothing secret is printed.
//
//   node scripts/s4_refusals.ts manifest <receipts>
//   CENTRAL_SEED_ACCESS_TOKEN=… node scripts/s4_refusals.ts baseline <origin> <receipts>
//   CENTRAL_SEED_ACCESS_TOKEN=… node scripts/s4_refusals.ts apply    <origin> <receipts> <case id>
//   CENTRAL_SEED_ACCESS_TOKEN=… node scripts/s4_refusals.ts migrate  <origin> <receipts> <case id>
//   CENTRAL_SEED_ACCESS_TOKEN=… node scripts/s4_refusals.ts undo     <origin> <receipts> <case id>
//
// Each phase exits 1 on any stop: apply when the cohort isn't the baseline,
// migrate unless it is refused with the case's named stop and no rows written,
// undo unless the cohort is unchanged since apply and restores to a
// byte-identical baseline.

import { isDeepStrictEqual } from "node:util";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { ownerFetch } from "../../../tools/central-migration/scripts/owner_transport.ts";
import {
  PRODUCTION,
  canonicalName,
} from "../../../tools/central-migration/scripts/worker_deploy.ts";
import {
  cohortKey,
  restoreObjects,
  s4Manifest,
  snapshotCohort,
  writeStatements,
  type CohortObject,
  type S4Call,
  type S4Manifest,
} from "./s4_cases.ts";

const [phase, ...rest] = process.argv.slice(2);
const usage =
  "usage: s4_refusals.ts manifest <receipts> | <baseline|apply|migrate|undo> <origin> <receipts> [case id]";

async function keep(path: string, value: unknown): Promise<void> {
  await writeFile(path, `${JSON.stringify(value, null, 1)}\n`, { mode: 0o600, flag: "wx" });
}
const read = async <T>(path: string): Promise<T> => JSON.parse(await readFile(path, "utf8")) as T;

function cohortOf(manifest: S4Manifest): CohortObject[] {
  const objects = new Map<string, CohortObject>();
  for (const object of [...manifest.baseline, ...manifest.cases.flatMap((entry) => entry.objects)])
    objects.set(cohortKey(object), { binding: object.binding, name: object.name });
  return [...objects.values()];
}

function transport(origin: string): S4Call {
  const url = new URL(origin);
  const host = canonicalName(url.hostname);
  if (
    url.protocol !== "https:" ||
    host === PRODUCTION.domain ||
    host.includes("collaboration-central.")
  )
    throw new Error(`refusing ${url.hostname}: S4 runs only against its own staging allocation`);
  const token = process.env.CENTRAL_SEED_ACCESS_TOKEN;
  if (token === undefined || token === "") throw new Error("CENTRAL_SEED_ACCESS_TOKEN is not set");
  return async (path, body) => {
    const response = await ownerFetch(new URL(path, url), token, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    const text = await response.text();
    return { status: response.status, body: text === "" ? null : (JSON.parse(text) as unknown) };
  };
}

function stop(reason: string): never {
  console.log(JSON.stringify({ phase, outcome: "stop", reason }));
  process.exit(1);
}

if (phase === "manifest") {
  const [receipts] = rest;
  if (receipts === undefined) throw new Error(usage);
  await mkdir(receipts, { recursive: true, mode: 0o700 });
  const manifest = s4Manifest();
  await keep(join(receipts, "manifest.json"), manifest);
  console.log(
    JSON.stringify({
      phase,
      cases: manifest.cases.map(({ id, row }) => ({ id, row })),
      cohort: cohortOf(manifest).length,
    }),
  );
} else {
  const [origin, receipts, caseId] = rest;
  if (
    origin === undefined ||
    receipts === undefined ||
    !["baseline", "apply", "migrate", "undo"].includes(phase ?? "")
  )
    throw new Error(usage);
  const manifest = await read<S4Manifest>(join(receipts, "manifest.json"));
  const objects = cohortOf(manifest);
  const call = transport(origin);
  if (phase === "baseline") {
    const rowsWritten = await writeStatements(call, manifest.baseline);
    await keep(join(receipts, "baseline.json"), await snapshotCohort(call, objects));
    console.log(JSON.stringify({ phase, rowsWritten, objects: objects.length }));
  } else {
    const entry = manifest.cases.find((candidate) => candidate.id === caseId);
    if (entry === undefined) throw new Error(`no case ${String(caseId)} in the manifest`);
    const directory = join(receipts, entry.id);
    await mkdir(directory, { recursive: true, mode: 0o700 });
    const baseline = await read<Record<string, unknown>>(join(receipts, "baseline.json"));
    if (phase === "apply") {
      const before = await snapshotCohort(call, objects);
      await keep(join(directory, "before.json"), before);
      if (!isDeepStrictEqual(before, baseline))
        stop(`${entry.id}: the cohort isn't the baseline before apply`);
      const rowsWritten = await writeStatements(call, entry.objects);
      await keep(join(directory, "applied.json"), await snapshotCohort(call, objects));
      console.log(JSON.stringify({ phase, case: entry.id, row: entry.row, rowsWritten }));
    } else if (phase === "migrate") {
      const answer = await call("/admin/v1/migration/migrate", entry.migrateBody);
      await keep(join(directory, "migrate.json"), answer);
      const body = answer.body as {
        kind?: string;
        rowsWritten?: number;
        stops?: { object: string; subject: string; reason: string }[];
      };
      const named = (body.stops ?? []).some(
        (found) =>
          found.object === entry.expected.object &&
          found.subject === entry.expected.subject &&
          new RegExp(entry.expected.reason).test(found.reason),
      );
      if (answer.status !== 409 || body.kind !== "stopped" || body.rowsWritten !== 0 || !named)
        stop(
          `${entry.id}: migrate answered ${answer.status} ${String(body.kind)}, rowsWritten ${String(body.rowsWritten)}, named ${named}`,
        );
      console.log(JSON.stringify({ phase, case: entry.id, outcome: "refused", stops: body.stops }));
    } else {
      const applied = await read<Record<string, unknown>>(join(directory, "applied.json"));
      const afterMigrate = await snapshotCohort(call, objects);
      await keep(join(directory, "after-migrate.json"), afterMigrate);
      if (!isDeepStrictEqual(afterMigrate, applied))
        stop(`${entry.id}: storage changed between apply and undo`);
      const rowsWritten = await restoreObjects(call, entry.objects, baseline as never);
      const afterUndo = await snapshotCohort(call, objects);
      await keep(join(directory, "after-undo.json"), afterUndo);
      if (!isDeepStrictEqual(afterUndo, baseline))
        stop(`${entry.id}: undo didn't restore the baseline byte for byte`);
      console.log(JSON.stringify({ phase, case: entry.id, outcome: "restored", rowsWritten }));
    }
  }
}
