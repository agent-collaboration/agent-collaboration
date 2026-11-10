/** Stream a signed public-API extractor to private evidence and validate its complete snapshot. */
import { spawn } from "node:child_process";
import { createReadStream } from "node:fs";
import { open, mkdir } from "node:fs/promises";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { z } from "zod";
import type { Artifact } from "../../../declaration.ts";
import { decode } from "../../../exact-json.ts";
import { privateDirectory, qualifyArtifact } from "../../../machines/owned-process.ts";
import { identifier, unsigned } from "./contracts.ts";
import { hashFile } from "./project-backups.ts";

const position = z.object({ epoch: unsigned, sequence: unsigned }).strict();
const schema = z.object({ version: unsigned, fingerprint: z.string().min(1) }).strict();
const copy = z
  .object({
    copy: identifier,
    role: z.enum(["authoritative", "follower"]),
    start: position,
    acked: position,
    needsSnapshot: z.boolean(),
  })
  .strict();
const header = z
  .object({
    kind: z.literal("seedSnapshotHeader"),
    database: identifier,
    holder: identifier,
    registration: z
      .object({
        Ready: z
          .object({
            lease: z
              .object({
                database: identifier,
                holder: identifier,
                epoch: unsigned,
              })
              .passthrough(),
          })
          .strict(),
      })
      .strict(),
    writerState: z.object({ state: z.enum(["writing", "paused"]), epoch: unsigned }).passthrough(),
    position,
    schema,
    copies: z.array(copy),
  })
  .strict();
const envelope = z.object({ kind: z.string() }).passthrough();
function comparePosition(left: z.infer<typeof position>, right: z.infer<typeof position>): number {
  if (left.epoch !== right.epoch) return left.epoch < right.epoch ? -1 : 1;
  return left.sequence === right.sequence ? 0 : left.sequence < right.sequence ? -1 : 1;
}
export function classifySeedHeader(
  raw: unknown,
  expected: { project: string; holder: string; lease: bigint },
) {
  const observed = header.parse(raw);
  const lease = observed.registration.Ready.lease;
  if (
    observed.database !== expected.project ||
    observed.holder !== expected.holder ||
    lease.database !== expected.project ||
    lease.holder !== expected.holder ||
    lease.epoch !== expected.lease ||
    lease.epoch !== observed.writerState.epoch ||
    observed.position.epoch > lease.epoch
  )
    throw new Error("seed header writer identity or fence mismatch");
  if (new Set(observed.copies.map((status) => status.copy)).size !== observed.copies.length)
    throw new Error("duplicate sender receiver");
  const authoritative = observed.copies.filter((status) => status.role === "authoritative");
  const [status] = authoritative;
  if (authoritative.length !== 1 || status === undefined)
    throw new Error("exactly one authoritative sender receiver required");
  if (
    comparePosition(status.start, status.acked) > 0 ||
    comparePosition(status.acked, observed.position) > 0
  )
    throw new Error("sender positions outside the head interval");
  return {
    observed,
    authoritative: status,
    hasTail: comparePosition(status.acked, observed.position) < 0,
    classification: status.needsSnapshot ? "needsSnapshot" : "readyForBaseline",
    retainedFloorObserved: false,
  };
}
function samePosition(left: z.infer<typeof position>, right: z.infer<typeof position>): boolean {
  return comparePosition(left, right) === 0;
}
function sameSchema(left: z.infer<typeof schema>, right: z.infer<typeof schema>): boolean {
  return left.version === right.version && left.fingerprint === right.fingerprint;
}
const pageEnvelope = z
  .object({
    page: z
      .object({
        database: identifier,
        position,
        schema,
        table: z.string(),
        rows: z.array(z.array(z.unknown())),
        schemaSteps: z.array(z.string()),
        next: z.unknown(),
      })
      .strict(),
  })
  .passthrough();
const manifestEnvelope = z
  .object({
    manifest: z
      .object({
        database: identifier,
        position,
        schema,
        tables: z.array(
          z.object({ table: z.string().min(1), rowCount: unsigned, digest: z.unknown() }).strict(),
        ),
      })
      .strict(),
  })
  .passthrough();
export async function validateSeedSnapshot(input: {
  path: string;
  expectedProject: string;
  expectedHolder: string;
  expectedLease: bigint;
  untouchedSha256: string;
}) {
  let observed: ReturnType<typeof classifySeedHeader> | undefined;
  let manifestSeen = false;
  let complete = false;
  let pages = 0;
  const counts = new Map<string, bigint>();
  const lines = createInterface({ input: createReadStream(input.path), crlfDelay: Infinity });
  for await (const line of lines) {
    if (complete || line.length > 8 * 1024 * 1024) throw new Error("invalid snapshot framing");
    const raw = decode(line);
    const record = envelope.parse(raw);
    if (record.kind === "seedSnapshotHeader") {
      if (observed !== undefined) throw new Error("duplicate snapshot header");
      observed = classifySeedHeader(raw, {
        project: input.expectedProject,
        holder: input.expectedHolder,
        lease: input.expectedLease,
      });
      continue;
    }
    if (observed === undefined) throw new Error("snapshot header missing");
    const pinned = observed.observed;
    if (record.kind === "seedSnapshotPage") {
      if (manifestSeen) throw new Error("snapshot page after manifest");
      const page = pageEnvelope.parse(raw).page;
      if (
        page.database !== pinned.database ||
        !samePosition(page.position, pinned.position) ||
        !sameSchema(page.schema, pinned.schema)
      )
        throw new Error("snapshot page differs from pinned head");
      if (page.table !== "")
        counts.set(page.table, (counts.get(page.table) ?? 0n) + BigInt(page.rows.length));
      else if (page.rows.length !== 0) throw new Error("unnamed snapshot rows");
      pages += 1;
    } else if (record.kind === "seedSnapshotManifest") {
      if (manifestSeen || pages === 0) throw new Error("missing pages or duplicate manifest");
      const manifest = manifestEnvelope.parse(raw).manifest;
      if (
        manifest.database !== pinned.database ||
        !samePosition(manifest.position, pinned.position) ||
        !sameSchema(manifest.schema, pinned.schema) ||
        new Set(manifest.tables.map((table) => table.table)).size !== manifest.tables.length ||
        manifest.tables.length !== counts.size ||
        manifest.tables.some((table) => counts.get(table.table) !== table.rowCount)
      )
        throw new Error("snapshot manifest or row counts differ");
      manifestSeen = true;
    } else if (record.kind === "seedSnapshotComplete") {
      const done = z
        .object({
          database: identifier,
          holder: identifier,
          position,
          untouchedBeforeSha256: z.string(),
          untouchedAfterSha256: z.string(),
          retainedFloor: z.null(),
          retainedFloorObserved: z.literal(false),
          currentRenewalObserved: z.literal(false),
        })
        .passthrough()
        .parse(raw);
      if (
        !manifestSeen ||
        done.database !== pinned.database ||
        done.holder !== pinned.holder ||
        !samePosition(done.position, pinned.position) ||
        done.untouchedBeforeSha256 !== input.untouchedSha256 ||
        done.untouchedAfterSha256 !== input.untouchedSha256
      )
        throw new Error("snapshot completion mismatch");
      complete = true;
    } else throw new Error("unknown snapshot record");
  }
  if (!complete || observed === undefined) throw new Error("incomplete snapshot output");
  return {
    ...observed,
    pages,
    tables: counts.size,
    rows: [...counts.values()].reduce((sum, value) => sum + value, 0n),
  };
}

export async function extractSeedSnapshot(input: {
  artifact: Artifact;
  scratch: string;
  untouched: { path: string; sha256: string };
  expectedProject: string;
  expectedHolder: string;
  expectedLease: bigint;
  evidenceDirectory: string;
  adoptJournal?: boolean;
}) {
  await qualifyArtifact(input.artifact);
  if ((await hashFile(input.untouched.path)) !== input.untouched.sha256)
    throw new Error("B changed before extraction");
  await mkdir(input.evidenceDirectory, { mode: 0o700 });
  await privateDirectory(input.evidenceDirectory);
  const path = join(input.evidenceDirectory, "snapshot.jsonl");
  const output = await open(path, "wx", 0o600);
  const errors = await open(join(input.evidenceDirectory, "stderr.txt"), "wx", 0o600);
  let exit: number | null;
  try {
    const child = spawn(
      input.artifact.path,
      [
        input.adoptJournal === true ? "--adopt-ledger-snapshot" : "--seed-snapshot",
        input.scratch,
        input.untouched.path,
        input.expectedProject,
        input.expectedHolder,
        input.expectedLease.toString(),
      ],
      {
        stdio: ["ignore", output.fd, errors.fd],
        signal: AbortSignal.timeout(300_000),
      },
    );
    exit = await new Promise<number | null>((resolve, reject): void => {
      child.once("error", reject);
      child.once("close", resolve);
    });
  } finally {
    await output.close();
    await errors.close();
  }
  if ((await hashFile(input.untouched.path)) !== input.untouched.sha256)
    throw new Error("B changed during extraction");
  await qualifyArtifact(input.artifact);
  if (exit !== 0)
    throw new Error(`snapshot extractor refused with exit ${exit}; inspect private stderr`);
  const observation = await validateSeedSnapshot({
    path,
    expectedProject: input.expectedProject,
    expectedHolder: input.expectedHolder,
    expectedLease: input.expectedLease,
    untouchedSha256: input.untouched.sha256,
  });
  return { path, sha256: await hashFile(path), ...observation };
}
