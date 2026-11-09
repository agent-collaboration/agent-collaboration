// The storage-level R7 checks every object shares: its tables are production's,
// its migration history is production's full set with production's checksums,
// no migration failed, and no journal operation is unfinished.

import { exactUnsigned } from "./exact_json.ts";
import { MAIN_SETS, hex, migrationChecksum } from "./schema_sets.ts";
import type { ReadReply, ReadRow } from "./storage_objects.ts";

export interface Stop {
  readonly object: string;
  readonly subject: string;
  readonly reason: string;
}

export const rows = (reply: ReadReply, table: string): readonly ReadRow[] =>
  reply.rows[table] ?? [];

/** A migration-record version: small, so a safe number when it is a canonical unsigned integer. */
const versionOf = (value: unknown): number | null => {
  const exact = exactUnsigned(value);
  return exact !== null && exact <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(exact) : null;
};

/** Production stored checksums as a JSON array of bytes (O object_bootstrap.rs:479–481). */
function decodeChecksum(text: unknown): Uint8Array | null {
  if (typeof text !== "string") return null;
  try {
    const bytes: unknown = JSON.parse(text);
    if (
      !Array.isArray(bytes) ||
      bytes.length !== 48 ||
      !bytes.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)
    ) {
      return null;
    }
    return Uint8Array.from(bytes as number[]);
  } catch {
    return null;
  }
}

export function checkShape(
  object: string,
  reply: ReadReply,
  expected: readonly string[],
  exact: boolean,
  stops: Stop[],
) {
  const present = new Set(reply.tables);
  const missing = expected.filter((table) => !present.has(table));
  const extra = exact ? reply.tables.filter((table) => !expected.includes(table)) : [];
  if (missing.length > 0 || extra.length > 0) {
    stops.push({
      object,
      subject: "storage shape",
      reason: `not production's shape: missing [${missing.join(", ")}], unexpected [${extra.join(", ")}]`,
    });
  }
}

export function checkHistory(
  object: string,
  reply: ReadReply,
  sets: Readonly<Record<string, readonly number[]>>,
  stops: Stop[],
) {
  const failures = rows(reply, "central_schema_migration_failures");
  if (failures.length > 0) {
    stops.push({
      object,
      subject: "migration failures",
      reason: `migration-failure rows present for ${failures.map((row) => String(row.corpus)).join(", ")}`,
    });
  }
  const recorded = new Map<string, number[]>();
  for (const row of rows(reply, "central_schema_migrations")) {
    const corpus = String(row.corpus);
    const version = versionOf(row.version);
    if (version === null || decodeChecksum(row.checksum) === null) {
      stops.push({ object, subject: `set ${corpus}`, reason: "unreadable migration record" });
      continue;
    }
    recorded.set(
      corpus,
      [...(recorded.get(corpus) ?? []), version].sort((a, b) => a - b),
    );
  }
  const wanted = Object.keys(sets).sort();
  const found = [...recorded.keys()].sort();
  for (const corpus of new Set([...wanted, ...found])) {
    const expected = sets[corpus] ?? [];
    const actual = recorded.get(corpus) ?? [];
    if (expected.join(",") !== actual.join(",")) {
      stops.push({
        object,
        subject: `set ${corpus}`,
        reason: `migration set is [${actual.join(",")}], production's full set is [${expected.join(",")}]`,
      });
    }
  }
}

export function checkJournal(object: string, reply: ReadReply, stops: Stop[]) {
  for (const row of rows(reply, "_fleet_sync_operations")) {
    if (row.state === "running") {
      stops.push({
        object,
        subject: `operation ${String(row.operation_id)}`,
        reason: `unfinished journal operation ${String(row.kind)}; let it finish on the old central`,
      });
    }
  }
}

/** Production's receiver records must already equal main's checksums (identical files). */
export async function receiverChecksumsMatch(reply: ReadReply): Promise<boolean> {
  for (const row of rows(reply, "central_schema_migrations")) {
    const migration = MAIN_SETS["fleet-sync-receiver"].find(
      (candidate) => candidate.version === versionOf(row.version),
    );
    const stored = decodeChecksum(row.checksum);
    if (migration === undefined || stored === null) return false;
    if (hex(stored) !== hex(await migrationChecksum(migration.sql))) return false;
  }
  return true;
}
