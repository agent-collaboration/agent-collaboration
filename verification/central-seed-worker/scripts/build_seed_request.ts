// Writes the staging seed's requests for every exported project, after
// checking each table's rows against its Rust-computed manifest digest: copy
// batches (each under BATCH_BYTES, phase 1) and one Registry request (phase 2).
// Writes only those files (mode 0600, in a fresh directory); it sends nothing.
//
//   node scripts/build_seed_request.ts <export manifest.json> <staging target>
//     <registry incarnation> <seeded-at ms> <expires-at ms> <out directory>

import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import type { CopiesRequest, RegistryRequest, SeedProject } from "../src/seed_contract.ts";
import { digestMismatches, projectFromSnapshotStream } from "./seed_request.ts";

/** Workers accept request bodies far larger; staying small keeps one Copy's write bounded. */
const BATCH_BYTES = 16 * 1024 * 1024;

const [manifestPath, target, registryIncarnation, seededAt, expiresAt, out] = process.argv.slice(2);
if (
  out === undefined ||
  target === undefined ||
  registryIncarnation === undefined ||
  seededAt === undefined ||
  expiresAt === undefined
) {
  throw new Error(
    "usage: build_seed_request.ts <manifest.json> <target> <incarnation> <seededAt> <expiresAt> <out dir>",
  );
}
await mkdir(out, { mode: 0o700 });
const manifest = JSON.parse(await readFile(manifestPath ?? "", "utf8")) as {
  records: { project: string; status: string; path: string }[];
};
const receivers = new Set<string>();
const batches: SeedProject[][] = [[]];
let batchBytes = 0;
let rows = 0;
let tables = 0;
const registry: RegistryRequest = { target, registryIncarnation, projects: [] };
for (const record of manifest.records) {
  if (record.status !== "exported")
    throw new Error(`project ${record.project} was not exported: ${record.status}`);
  const exported = projectFromSnapshotStream(await readFile(record.path, "utf8"), {
    seededAt,
    expiresAt,
  });
  const { project } = exported;
  if (project.database !== record.project)
    throw new Error(`stream for ${record.project} names ${project.database}`);
  const mismatched = digestMismatches(project);
  if (mismatched.length > 0)
    throw new Error(`project ${record.project}: digests differ for ${mismatched.join(", ")}`);
  receivers.add(exported.receiver);
  rows += exported.rows;
  tables += project.manifest.length;
  const size = JSON.stringify(project).length;
  if (batchBytes + size > BATCH_BYTES && (batches.at(-1)?.length ?? 0) > 0) {
    batches.push([]);
    batchBytes = 0;
  }
  batches.at(-1)?.push(project);
  batchBytes += size;
  const { database, holder, leaseNumber } = project;
  (registry.projects as unknown[]).push({ database, holder, leaseNumber, seededAt, expiresAt });
}
const files: string[] = [];
for (const [index, projects] of batches.entries()) {
  const file = join(out, `copies-${String(index + 1).padStart(3, "0")}.json`);
  const request: CopiesRequest = { target, projects };
  await writeFile(file, JSON.stringify(request), { mode: 0o600 });
  files.push(file);
}
const registryFile = join(out, "registry.json");
await writeFile(registryFile, JSON.stringify(registry), { mode: 0o600 });
console.log(
  JSON.stringify({
    projects: registry.projects.length,
    tables,
    rows,
    // The staging authoritative receiver configuration must use exactly this one (spec §1, C).
    receivers: [...receivers],
    copies: files,
    registry: registryFile,
  }),
);
