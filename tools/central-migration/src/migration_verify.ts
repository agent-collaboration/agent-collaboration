// Verify (PD5): read back every carried record and every copy's state and
// compare each with what its old source says it must be. The old sources stay
// in place: the registry object, the Fleet tables main doesn't reuse, and the
// renamed legacy_ tables.

import { exactUnsigned, parseExact, stringifyExact } from "./exact_json.ts";
import { convertFleetGraph, isUuidV7 } from "./fleet_conversion.ts";
import type { NewRow } from "./fleet_conversion.ts";
import {
  MAIN_SETS,
  LEASE_TABLES,
  hex,
  ledgerTable,
  legacyName,
  migrationChecksum,
} from "./schema_sets.ts";
import type { MigrationSetName } from "./schema_sets.ts";
import type { ReadReply, ReadRow } from "./storage_objects.ts";

export interface Check {
  readonly name: string;
  readonly ok: boolean;
  readonly detail?: string;
}

const CARRIED = [
  "fleet_workspaces",
  "fleet_workspace_repository_refs",
  "fleet_boards",
  "fleet_project_directory",
  "fleet_workspace_adrs",
  "fleet_adr_repository_files",
  "fleet_repository_attachments",
] as const;
const CHILD_IDS: Partial<Record<(typeof CARRIED)[number], string>> = {
  fleet_workspace_repository_refs: "row_id",
  fleet_adr_repository_files: "id",
};
const FLEET_SETS = ["central-fleet", "central-registry", "fleet-sync-lease"] as const;

export const FLEET_VERIFY_READ = [
  "workspaces",
  "boards",
  "project_directory",
  "workspace_adrs",
  "repository_attachments",
  legacyName("fleet_feed_state"),
  "lease_incarnation_fence",
  ...CARRIED,
  "fleet_feed_state",
  "machine_enrollments",
  "lease_incarnation",
  ...LEASE_TABLES,
  ...FLEET_SETS.map(ledgerTable),
];
export const COPY_VERIFY_READ = ["_fleet_sync_copy", ledgerTable("fleet-sync-receiver")];

const rows = (reply: ReadReply, table: string): readonly ReadRow[] => reply.rows[table] ?? [];
const canonical = (value: unknown): string => stringifyExact(value);
const sorted = (list: readonly unknown[]): string[] => list.map(canonical).sort();

function sameRows(name: string, actual: readonly unknown[], expected: readonly unknown[]): Check {
  const a = sorted(actual);
  const e = sorted(expected);
  const ok = a.length === e.length && a.every((row, index) => row === e[index]);
  return ok
    ? { name, ok }
    : {
        name,
        ok,
        detail: `found ${a.length} rows, expected ${e.length}; first difference: ${a.find((row) => !e.includes(row)) ?? e.find((row) => !a.includes(row))}`,
      };
}

async function expectedLedger(set: MigrationSetName, installedOn: string) {
  return Promise.all(
    MAIN_SETS[set].map(async (migration) => ({
      version: migration.version,
      description: migration.description,
      installed_on: installedOn,
      success: 1,
      checksum: { blob: hex(await migrationChecksum(migration.sql)) },
      execution_time: 0,
    })),
  );
}

export async function verifyMigration(
  registry: ReadReply,
  fleet: ReadReply,
  copies: Readonly<Record<string, ReadReply>>,
): Promise<{ readonly ok: boolean; readonly checks: readonly Check[] }> {
  const checks: Check[] = [];
  const ledger = rows(fleet, ledgerTable("central-fleet"));
  const installedOn = String(ledger[0]?.installed_on ?? "");
  const runTime = Date.parse(`${installedOn.replace(" ", "T")}Z`);
  if (Number.isNaN(runTime)) {
    return {
      ok: false,
      checks: [{ name: "fleet migrated", ok: false, detail: "no central-fleet records" }],
    };
  }
  const runTimeText = new Date(runTime).toISOString();

  // Migration records: main's versions and checksums, per set.
  for (const set of FLEET_SETS) {
    checks.push(
      sameRows(
        `fleet records ${set}`,
        rows(fleet, ledgerTable(set)),
        await expectedLedger(set, installedOn),
      ),
    );
  }

  // The Fleet graph, recomputed from the old tables still in place.
  const expected = convertFleetGraph(
    {
      workspaces: rows(fleet, "workspaces"),
      boards: rows(fleet, "boards"),
      project_directory: rows(fleet, "project_directory"),
      workspace_adrs: rows(fleet, "workspace_adrs"),
      repository_attachments: rows(fleet, "repository_attachments"),
    },
    runTime,
    () => "",
  );
  for (const table of CARRIED) {
    const idColumn = CHILD_IDS[table];
    // Columns the migration doesn't write (main's later projections) must be empty.
    const written = new Set(Object.keys(expected[table][0] ?? {}));
    const actual = rows(fleet, table).map((fullRow) => {
      const row = Object.fromEntries(
        Object.entries(fullRow).filter(([column]) => written.has(column)),
      );
      const unwritten = Object.entries(fullRow).filter(
        ([column, value]) => !written.has(column) && value !== null,
      );
      if (unwritten.length > 0)
        checks.push({
          name: `${table} unwritten columns`,
          ok: false,
          detail: canonical(unwritten),
        });
      if (idColumn === undefined) return row;
      const { [idColumn]: id, ...rest } = row;
      if (!isUuidV7(id))
        checks.push({ name: `${table} ids`, ok: false, detail: `${String(id)} is not UUIDv7` });
      return rest;
    });
    const wanted = expected[table].map((row: NewRow) => {
      if (idColumn === undefined) return row;
      const { [idColumn]: _id, ...rest } = row;
      return rest;
    });
    checks.push(sameRows(table, actual, wanted));
  }

  // Feed head and floor at the old head.
  const oldFeed = rows(fleet, legacyName("fleet_feed_state"))[0];
  checks.push(
    sameRows(
      "fleet_feed_state",
      rows(fleet, "fleet_feed_state"),
      oldFeed === undefined
        ? []
        : [
            {
              state_id: oldFeed.state_id,
              name: oldFeed.name,
              last_position: oldFeed.last_position,
              retained_through: oldFeed.last_position,
              created_at: runTime,
              updated_at: runTime,
            },
          ],
    ),
  );

  // Machines, byte-exact apart from the renamed columns and the run time.
  checks.push(
    sameRows(
      "machine_enrollments",
      rows(fleet, "machine_enrollments"),
      rows(registry, "machine_enrollments").map((row) => ({
        machine_id: row.peer_id,
        service_id: row.service_id,
        machine_label: row.machine_label,
        access_client_id: row.access_client_id,
        credential_hash: row.credential_hash,
        state: row.state,
        expires_at: row.expires_at,
        last_seen_at: row.last_seen,
        last_revocation_operation_id: row.last_revocation_operation,
        last_enrollment_operation_id: row.last_enrollment_operation,
        created_at: runTimeText,
        updated_at: runTimeText,
      })),
    ),
  );

  // Lease incarnation: exactly the larger of production's two counters, compared as
  // integers from their stored text so no rounding can hide a difference.
  const counters = [
    ...rows(registry, "registry_lease_incarnation"),
    ...rows(fleet, "lease_incarnation_fence"),
  ].map((row) => exactUnsigned(row.incarnation));
  const incarnation = rows(fleet, "lease_incarnation");
  const largest = counters.reduce<bigint | null>(
    (larger, counter) =>
      counter === null || larger === null ? null : counter > larger ? counter : larger,
    counters.length === 0 ? null : 0n,
  );
  const stored = exactUnsigned(incarnation[0]?.incarnation);
  checks.push({
    name: "lease_incarnation",
    ok:
      counters.length === 0
        ? incarnation.length === 0
        : largest !== null &&
          incarnation.length === 1 &&
          stored === largest &&
          incarnation[0]?.name === "fleet",
    detail: `stored ${canonical(incarnation)}, production counters ${canonical(counters)}`,
  });

  // Library lease tables, byte-exact.
  for (const table of LEASE_TABLES)
    checks.push(sameRows(table, rows(fleet, table), rows(registry, table)));

  // Each copy: its receiver records, stamped when that copy committed. A copy
  // migrated by an earlier, interrupted run keeps its own earlier stamp; no copy
  // can be stamped after the Fleet, which commits last.
  const leases = new Map(
    rows(registry, "_fleet_sync_leases").map((row) => [
      String(row.database),
      parseExact(String(row.record)) as { holder?: string; epoch?: number | bigint },
    ]),
  );
  for (const [project, copy] of Object.entries(copies)) {
    const records = rows(copy, ledgerTable("fleet-sync-receiver"));
    const copyStamp = String(records[0]?.installed_on ?? "");
    const stampOk =
      /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(copyStamp) && copyStamp <= installedOn;
    if (!stampOk) {
      checks.push({
        name: `copy ${project} records`,
        ok: false,
        detail: `stamp ${copyStamp} is not a whole-second time at or before the Fleet's ${installedOn}`,
      });
      continue;
    }
    checks.push(
      sameRows(
        `copy ${project} records`,
        records,
        await expectedLedger("fleet-sync-receiver", copyStamp),
      ),
    );
    const lease = leases.get(project);
    if (lease?.holder !== undefined) {
      const record = parseExact(String(rows(copy, "_fleet_sync_copy")[0]?.record ?? "null")) as {
        state?: string;
        copy?: { fence?: number | bigint; holder?: string };
      } | null;
      const fence = exactUnsigned(record?.copy?.fence) ?? 0n;
      const leaseEpoch = exactUnsigned(lease.epoch) ?? 0n;
      const agrees =
        record?.state === "following" &&
        fence <= leaseEpoch &&
        (fence !== leaseEpoch || record.copy?.holder === lease.holder);
      checks.push({ name: `copy ${project} state`, ok: agrees, detail: canonical(record) });
    }
  }
  return { ok: checks.every((check) => check.ok), checks };
}
