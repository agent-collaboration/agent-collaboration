// Builds the seed Worker's request from the Verifier's snapshot exports
// (crates/board-staging-cutover/examples/retained_writer_observation, one
// snapshot.jsonl per project), reading every number losslessly. It checks
// what the old receiver's seed publication checks of its input
// (seed_publication.rs:30–57): every page and the manifest share the header's
// database, position and schema, and every page carries the same schema steps.

import { LosslessNumber, parse } from "lossless-json";
import { checkProject, type SeedProject, type TransportValue } from "../src/seed_contract.ts";
import { serdeReal, tableDigest } from "../src/table_digest.ts";
import { transportToSeed } from "../src/seed_contract.ts";

const text = (value: unknown, where: string): string => {
  if (value instanceof LosslessNumber) return value.toString();
  if (typeof value === "string") return value;
  throw new Error(`${where}: expected a number or text`);
};

function transport(wire: unknown, where: string): TransportValue {
  const { kind, value } = (wire ?? {}) as { kind?: unknown; value?: unknown };
  switch (kind) {
    case "null":
      return { kind: "null" };
    case "integer":
      return { kind: "integer", value: text(value, where) };
    case "real":
      if (value === "positiveInfinity" || value === "negativeInfinity")
        return { kind: "real", value };
      return { kind: "real", value: serdeReal(Number(text(value, where))) };
    case "text":
      if (typeof value !== "string") throw new Error(`${where}: text must be a string`);
      return { kind: "text", value };
    case "blob":
      if (typeof value !== "string") throw new Error(`${where}: blob must be base64`);
      return { kind: "blob", value };
    default:
      throw new Error(`${where}: unknown SqlValue kind ${String(kind)}`);
  }
}

const position = (value: unknown, where: string) => {
  const { epoch, sequence } = (value ?? {}) as { epoch?: unknown; sequence?: unknown };
  return { epoch: text(epoch, `${where}.epoch`), sequence: text(sequence, `${where}.sequence`) };
};
const same = (left: unknown, right: unknown): boolean =>
  JSON.stringify(left) === JSON.stringify(right);

export interface ExportFacts {
  readonly seededAt: string;
  readonly expiresAt: string;
}
export interface ExportedProject {
  readonly project: SeedProject;
  /** C: the authoritative receiver the Mac's sender is registered with. */
  readonly receiver: string;
  readonly rows: number;
}

/** One project's request entry from its snapshot.jsonl text. */
export function projectFromSnapshotStream(stream: string, facts: ExportFacts): ExportedProject {
  const lines = stream
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => parse(line) as Record<string, unknown>);
  const header = lines.find((line) => line.kind === "seedSnapshotHeader");
  const manifestLine = lines.find((line) => line.kind === "seedSnapshotManifest");
  const pages = lines
    .filter((line) => line.kind === "seedSnapshotPage")
    .map((line) => line.page as Record<string, unknown>);
  if (header === undefined || manifestLine === undefined || pages.length === 0) {
    throw new Error("snapshot stream lacks its header, pages or manifest");
  }
  const database = String(header.database);
  const where = `project ${database}`;
  const lease = (header.registration as { Ready?: { lease?: Record<string, unknown> } }).Ready
    ?.lease;
  if (lease === undefined) throw new Error(`${where}: registration is not Ready`);
  if (String(lease.holder) !== String(header.holder) || String(lease.database) !== database) {
    throw new Error(`${where}: lease holder or database differs from the header`);
  }
  const authoritative = (header.copies as Record<string, unknown>[]).filter(
    (copy) => copy.role === "authoritative",
  );
  const [sender] = authoritative;
  if (authoritative.length !== 1 || sender === undefined)
    throw new Error(`${where}: expected one authoritative copy`);
  if (sender.needsSnapshot !== false)
    throw new Error(`${where}: the authoritative sender needs a snapshot`);
  const head = position(header.position, `${where}.position`);
  const schema = header.schema as { version: unknown; fingerprint: unknown };
  const first = pages[0] ?? {};
  for (const [index, page] of pages.entries()) {
    if (
      String(page.database) !== database ||
      !same(position(page.position, "page"), head) ||
      !same(page.schema, header.schema) ||
      !same(page.schemaSteps, first.schemaSteps)
    ) {
      throw new Error(`${where}: page ${index} header differs from the snapshot's`);
    }
  }
  const manifest = manifestLine.manifest as Record<string, unknown>;
  if (
    String(manifest.database) !== database ||
    !same(position(manifest.position, "manifest"), head) ||
    !same(manifest.schema, header.schema)
  ) {
    throw new Error(`${where}: manifest header differs from the snapshot's`);
  }
  let rowCount = 0;
  const project: SeedProject = {
    database,
    holder: String(header.holder),
    leaseNumber: text(lease.epoch, `${where}.lease.epoch`),
    start: position(sender.start, `${where}.start`),
    head,
    seededAt: facts.seededAt,
    expiresAt: facts.expiresAt,
    schema: {
      version: text(schema.version, `${where}.schema.version`),
      fingerprint: String(schema.fingerprint),
    },
    schemaSteps: (first.schemaSteps as unknown[]).map(String),
    pages: pages.map((page) => {
      const rows = (page.rows as unknown[][]).map((row, rowIndex) =>
        row.map((value, column) =>
          transport(value, `${where}.${String(page.table)}[${rowIndex}][${column}]`),
        ),
      );
      rowCount += rows.length;
      return { table: String(page.table), rows };
    }),
    manifest: (manifest.tables as Record<string, unknown>[]).map((entry) => ({
      table: String(entry.table),
      rowCount: text(entry.rowCount, `${where}.manifest.rowCount`),
      digest: String(entry.digest),
    })),
  };
  checkProject(project);
  return { project, receiver: String(sender.copy), rows: rowCount };
}

/** The same input check the Worker runs: each table's rows hash to its manifest digest. */
export function digestMismatches(project: SeedProject): string[] {
  return project.manifest.flatMap((entry) => {
    const rows = project.pages
      .filter((page) => page.table === entry.table)
      .flatMap((page) => page.rows)
      .map((row, index) =>
        row.map((value, column) => transportToSeed(value, `${entry.table}[${index}][${column}]`)),
      );
    return tableDigest(rows) === entry.digest ? [] : [entry.table];
  });
}
