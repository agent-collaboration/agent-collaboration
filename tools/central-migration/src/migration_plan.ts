// Inventory and migrate share one pure decision: read every old record, check
// it (R7), and either stop with every contradiction named, or produce each
// object's single transaction of statements. Nothing here touches storage.

import { Rejected, convertFleetGraph, insertStatement, isUuidV7 } from "./fleet_conversion.ts";
import type { CarriedFleetRows, NewRow } from "./fleet_conversion.ts";
import type { ReadRow } from "./storage_objects.ts";
import {
  COPY_RENAMED,
  FLEET_RENAMED,
  LEASE_TABLES,
  MAIN_SETS,
  PRODUCTION_SETS,
  PRODUCTION_TABLES,
  hex,
  ledgerDdl,
  ledgerTable,
  legacyName,
  migrationChecksum,
  type MigrationSetName,
} from "./schema_sets.ts";
import { SCHEMA_SOURCES } from "./generated/schema_sources.ts";
import type { ReadReply, Statement } from "./storage_objects.ts";

export interface Stop {
  readonly object: string;
  readonly subject: string;
  readonly reason: string;
}

export interface ProjectReport {
  readonly project: string;
  readonly holder: string | null;
  readonly leaseNumber: number | null;
  readonly lifecycle: string;
  readonly directory: {
    readonly workspace: string;
    readonly board: string;
    readonly name: string;
  } | null;
  readonly copy: {
    readonly state: string;
    readonly fence: number | null;
    readonly holder: string | null;
  } | null;
  readonly action: "carry" | "stop";
}

export interface MigrationPlan {
  readonly runTime: number;
  readonly stops: readonly Stop[];
  readonly projects: readonly ProjectReport[];
  readonly counts: Readonly<Record<string, number>>;
  /** Statements per object; empty when any stop was found. */
  readonly fleet: readonly Statement[];
  readonly copies: Readonly<Record<string, readonly Statement[]>>;
}

export interface OldStorage {
  readonly registry: ReadReply;
  readonly fleet: ReadReply;
  readonly copies: Readonly<Record<string, ReadReply>>;
}

/** Tables each object's read must return. */
export const REGISTRY_READ = [
  ...LEASE_TABLES,
  "machine_enrollments",
  "registry_lease_incarnation",
  "central_schema_migrations",
  "central_schema_migration_failures",
  "_fleet_sync_operations",
];
export const FLEET_READ = [
  "workspaces",
  "boards",
  "project_directory",
  "workspace_adrs",
  "repository_attachments",
  "fleet_feed_state",
  "lease_incarnation_fence",
  "central_schema_migrations",
  "central_schema_migration_failures",
  "_fleet_sync_operations",
];
export const COPY_READ = [
  "_fleet_sync_copy",
  "central_schema_migrations",
  "central_schema_migration_failures",
];

const rows = (reply: ReadReply, table: string): readonly ReadRow[] => reply.rows[table] ?? [];

function json(value: unknown): Record<string, unknown> | null {
  if (typeof value !== "string") return null;
  try {
    const parsed: unknown = JSON.parse(value);
    return typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}
const asNumber = (value: unknown): number | null =>
  typeof value === "number" && Number.isSafeInteger(value)
    ? value
    : typeof value === "string" && /^\d+$/.test(value)
      ? Number(value)
      : null;

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

function checkShape(
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

function checkHistory(
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
    const version = asNumber(row.version);
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

function checkJournal(object: string, reply: ReadReply, stops: Stop[]) {
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

/** UUIDv7 ids, fresh per child row (data tables follow the UUIDv7 rule). */
export function uuidV7(nowMillis: number): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  let time = BigInt(nowMillis);
  for (let index = 5; index >= 0; index -= 1) {
    bytes[index] = Number(time & 0xffn);
    time >>= 8n;
  }
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x70;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  const value = hex(bytes);
  return `${value.slice(0, 8)}-${value.slice(8, 12)}-${value.slice(12, 16)}-${value.slice(16, 20)}-${value.slice(20)}`;
}

const MILLIS_RFC3339 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,3})?(Z|[+-]\d{2}:\d{2})$/;

/** PD3: an enrollment in main's shape, or the reason main's decoder would reject it. */
function enrollment(row: ReadRow, runTimeText: string): NewRow | string {
  const peer = row.peer_id;
  if (!isUuidV7(peer)) return "machine id is not a canonical UUIDv7";
  for (const field of [
    "service_id",
    "machine_label",
    "access_client_id",
    "credential_hash",
    "expires_at",
  ]) {
    if (typeof row[field] !== "string" || row[field] === "") return `${field} missing`;
  }
  const hash = String(row.credential_hash);
  if (hash.trim() !== hash || /\p{Cc}/u.test(hash))
    return "credential hash has whitespace or control characters";
  if (row.state !== "enrolled" && row.state !== "revoked")
    return `state ${String(row.state)} is neither enrolled nor revoked`;
  if (Number.isNaN(Date.parse(String(row.expires_at)))) return "expires_at is not a time";
  if (
    row.last_seen !== null &&
    !(typeof row.last_seen === "string" && MILLIS_RFC3339.test(row.last_seen))
  ) {
    return "last_seen is not millisecond RFC 3339";
  }
  for (const field of ["last_revocation_operation", "last_enrollment_operation"]) {
    if (row[field] !== null && !isUuidV7(row[field])) return `${field} is not a canonical UUIDv7`;
  }
  return {
    machine_id: peer,
    service_id: String(row.service_id),
    machine_label: String(row.machine_label),
    access_client_id: String(row.access_client_id),
    credential_hash: hash,
    state: String(row.state),
    expires_at: String(row.expires_at),
    last_seen_at: (row.last_seen as string | null) ?? null,
    last_revocation_operation_id: (row.last_revocation_operation as string | null) ?? null,
    last_enrollment_operation_id: (row.last_enrollment_operation as string | null) ?? null,
    created_at: runTimeText,
    updated_at: runTimeText,
  };
}

async function setRecords(set: MigrationSetName, installedOn: string): Promise<Statement[]> {
  const statements: Statement[] = [{ sql: ledgerDdl(set), bindings: [] }];
  for (const migration of MAIN_SETS[set]) {
    statements.push({
      sql: `INSERT INTO ${ledgerTable(set)}(version,description,installed_on,success,checksum,execution_time) VALUES(?,?,?,1,unhex(?),0)`,
      bindings: [
        migration.version,
        migration.description,
        installedOn,
        hex(await migrationChecksum(migration.sql)),
      ],
    });
  }
  return statements;
}

const TABLE_ORDER: (keyof CarriedFleetRows)[] = [
  "fleet_workspaces",
  "fleet_workspace_repository_refs",
  "fleet_boards",
  "fleet_project_directory",
  "fleet_workspace_adrs",
  "fleet_adr_repository_files",
  "fleet_repository_attachments",
];

/**
 * The whole decision. `runTime` is one recorded instant, whole seconds, used
 * for every created_at, updated_at and installed_on the run writes.
 */
export async function planMigration(old: OldStorage, runTime: number): Promise<MigrationPlan> {
  const stops: Stop[] = [];
  const runTimeText = new Date(runTime).toISOString();
  const installedOn = runTimeText.replace("T", " ").slice(0, 19);
  const { registry, fleet } = old;

  checkShape("registry", registry, PRODUCTION_TABLES.registry, true, stops);
  checkShape("fleet", fleet, PRODUCTION_TABLES.fleet, true, stops);
  checkHistory("registry", registry, PRODUCTION_SETS.registry, stops);
  checkHistory("fleet", fleet, PRODUCTION_SETS.fleet, stops);
  checkJournal("registry", registry, stops);
  checkJournal("fleet", fleet, stops);

  // Machines (PD3).
  const machines = new Map<string, NewRow>();
  for (const row of rows(registry, "machine_enrollments")) {
    const converted = enrollment(row, runTimeText);
    if (typeof converted === "string") {
      stops.push({
        object: "registry",
        subject: `machine ${String(row.peer_id)}`,
        reason: converted,
      });
    } else {
      machines.set(String(converted.machine_id), converted);
    }
  }

  // Projects: registry record, lease, directory entry and copy (R7).
  const leases = new Map(
    rows(registry, "_fleet_sync_leases").map((row) => [String(row.database), json(row.record)]),
  );
  const directory = new Map(
    rows(fleet, "project_directory").map((row) => [String(row.project_id), row]),
  );
  const projects: ProjectReport[] = [];
  for (const transfer of rows(registry, "_fleet_sync_transfers")) {
    if (transfer.outcome === null) {
      stops.push({
        object: "registry",
        subject: `project ${String(transfer.database)}`,
        reason: "holder change in progress; let it finish on the old central",
      });
    }
  }
  for (const attempt of rows(registry, "_fleet_sync_lifecycle_attempts")) {
    if (attempt.outcome === null) {
      stops.push({
        object: "registry",
        subject: `project ${String(attempt.database)}`,
        reason: "lifecycle operation in progress; let it finish on the old central",
      });
    }
  }
  const registered = new Set<string>();
  for (const databaseRow of rows(registry, "_fleet_sync_databases")) {
    const project = String(databaseRow.database);
    registered.add(project);
    const subject = `project ${project}`;
    const before = stops.length;
    const record = json(databaseRow.record);
    const lifecycle = String(
      (record?.lifecycle as { state?: unknown } | undefined)?.state ?? "unreadable",
    );
    if (lifecycle === "retiring")
      stops.push({
        object: "registry",
        subject,
        reason: "retirement in progress; let it finish on the old central",
      });
    const lease = leases.get(project) ?? null;
    const holder = typeof lease?.holder === "string" ? lease.holder : null;
    const leaseNumber = asNumber(lease?.epoch);
    if (holder !== null && !machines.has(holder)) {
      stops.push({
        object: "registry",
        subject: `machine ${holder}`,
        reason: `holds ${project} but has no enrollment`,
      });
    }
    const entry = directory.get(project) ?? null;
    if (holder !== null && entry === null)
      stops.push({ object: "fleet", subject, reason: "held project has no directory entry" });
    const copyReply = old.copies[project];
    let copy: ProjectReport["copy"] = null;
    if (copyReply === undefined) {
      stops.push({ object: `copy ${project}`, subject, reason: "copy was not read" });
    } else {
      checkShape(`copy ${project}`, copyReply, PRODUCTION_TABLES.copyRequired, false, stops);
      checkHistory(`copy ${project}`, copyReply, PRODUCTION_SETS.copy, stops);
      if (!(await receiverChecksumsMatch(copyReply))) {
        stops.push({
          object: `copy ${project}`,
          subject: "set fleet-sync-receiver",
          reason: "stored receiver checksums differ from main's",
        });
      }
      const copyRecord = json(rows(copyReply, "_fleet_sync_copy")[0]?.record);
      const state = String(copyRecord?.state ?? "missing");
      const status = (state === "following" ? copyRecord?.copy : null) as Record<
        string,
        unknown
      > | null;
      const fence = asNumber(status?.fence);
      const copyHolder = typeof status?.holder === "string" ? status.holder : null;
      copy = { state, fence, holder: copyHolder };
      // A retired copy is preserved only beside a retired registry record (PD5, 81d442e3).
      if ((state === "retired") !== (lifecycle === "retired")) {
        stops.push({
          object: `copy ${project}`,
          subject,
          reason: `copy is ${state} but the registry lifecycle is ${lifecycle}`,
        });
      }
      if (holder !== null) {
        if (state !== "following") {
          stops.push({
            object: `copy ${project}`,
            subject,
            reason: `copy is ${state}, not a live Following copy`,
          });
        } else if (fence !== null && leaseNumber !== null && fence > leaseNumber) {
          stops.push({
            object: `copy ${project}`,
            subject,
            reason: `copy is at lease ${fence}, above the registry's ${leaseNumber}`,
          });
        } else if (fence === leaseNumber && copyHolder !== holder) {
          stops.push({
            object: `copy ${project}`,
            subject,
            reason: `copy holder ${String(copyHolder)} differs from the registry's ${holder} at lease ${leaseNumber}`,
          });
        }
      }
    }
    projects.push({
      project,
      holder,
      leaseNumber,
      lifecycle,
      directory:
        entry === null
          ? null
          : {
              workspace: String(entry.workspace_id),
              board: String(entry.board_id),
              name: String(entry.name),
            },
      copy,
      action: stops.length === before ? "carry" : "stop",
    });
  }
  for (const project of directory.keys()) {
    if (!registered.has(project)) {
      stops.push({
        object: "fleet",
        subject: `project ${project}`,
        reason: "directory entry has no registry record",
      });
    }
  }

  // The Fleet graph (PD4); any value main rejects stops the run.
  let graph: CarriedFleetRows | null = null;
  try {
    graph = convertFleetGraph(
      {
        workspaces: rows(fleet, "workspaces"),
        boards: rows(fleet, "boards"),
        project_directory: rows(fleet, "project_directory"),
        workspace_adrs: rows(fleet, "workspace_adrs"),
        repository_attachments: rows(fleet, "repository_attachments"),
      },
      runTime,
      () => uuidV7(runTime),
    );
  } catch (error) {
    if (!(error instanceof Rejected)) throw error;
    stops.push({ object: "fleet", subject: "carried row", reason: error.message });
  }
  const feedState = rows(fleet, "fleet_feed_state");
  if (feedState.length !== 1)
    stops.push({
      object: "fleet",
      subject: "feed state",
      reason: `expected one feed state row, found ${feedState.length}`,
    });

  const counts: Record<string, number> = {
    projects: projects.length,
    machines: machines.size,
    ...(graph === null
      ? {}
      : Object.fromEntries(TABLE_ORDER.map((table) => [table, graph[table].length]))),
  };
  if (stops.length > 0 || graph === null) {
    return { runTime, stops, projects, counts, fleet: [], copies: {} };
  }

  // Fleet: rename same-name tables, create main's sets, carry, then record.
  const statements: Statement[] = FLEET_RENAMED.map((table) => ({
    sql: `ALTER TABLE ${table} RENAME TO ${legacyName(table)}`,
    bindings: [],
  }));
  for (const set of ["central-fleet", "central-registry", "fleet-sync-lease"] as const) {
    for (const migration of MAIN_SETS[set]) statements.push({ sql: migration.sql, bindings: [] });
  }
  for (const table of TABLE_ORDER)
    for (const row of graph[table]) statements.push(insertStatement(table, row));
  const [head] = feedState;
  const lastPosition = asNumber(head?.last_position) ?? 0;
  // Head and floor both at the old head (PD4): a Router behind it sees an
  // expired cursor and rebuilds; one at it reads an empty page.
  statements.push(
    insertStatement("fleet_feed_state", {
      state_id: String(head?.state_id),
      name: String(head?.name),
      last_position: lastPosition,
      retained_through: lastPosition,
      created_at: runTime,
      updated_at: runTime,
    }),
  );
  for (const machine of machines.values())
    statements.push(insertStatement("machine_enrollments", machine));
  const incarnations = [
    ...rows(registry, "registry_lease_incarnation").map((row) => asNumber(row.incarnation)),
    ...rows(fleet, "lease_incarnation_fence").map((row) => asNumber(row.incarnation)),
  ].filter((value): value is number => value !== null);
  if (incarnations.length > 0) {
    statements.push(
      insertStatement("lease_incarnation", {
        row_id: uuidV7(runTime),
        name: "fleet",
        incarnation: Math.max(...incarnations),
        created_at: runTimeText,
        updated_at: runTimeText,
      }),
    );
  }
  // The library lease tables, byte-exact at production's shape (PD3).
  for (const table of LEASE_TABLES) {
    for (const row of rows(registry, table)) statements.push(insertStatement(table, row as NewRow));
  }
  for (const set of ["central-fleet", "central-registry", "fleet-sync-lease"] as const) {
    statements.push(...(await setRecords(set, installedOn)));
  }

  // Each copy: its failure table to main's shape, its receiver records to the
  // standard table. Project tables and the library's hook ledger stay as they are.
  const copies: Record<string, Statement[]> = {};
  const receiverRecords = await setRecords("fleet-sync-receiver", installedOn);
  for (const project of Object.keys(old.copies)) {
    copies[project] = [
      ...COPY_RENAMED.map((table) => ({
        sql: `ALTER TABLE ${table} RENAME TO ${legacyName(table)}`,
        bindings: [],
      })),
      { sql: SCHEMA_SOURCES.main.migrationFailures, bindings: [] },
      ...receiverRecords,
    ];
  }
  return { runTime, stops, projects, counts, fleet: statements, copies };
}

/** Production's receiver records must already equal main's checksums (identical files). */
export async function receiverChecksumsMatch(reply: ReadReply): Promise<boolean> {
  for (const row of rows(reply, "central_schema_migrations")) {
    const migration = MAIN_SETS["fleet-sync-receiver"].find(
      (candidate) => candidate.version === asNumber(row.version),
    );
    const stored = decodeChecksum(row.checksum);
    if (migration === undefined || stored === null) return false;
    if (hex(stored) !== hex(await migrationChecksum(migration.sql))) return false;
  }
  return true;
}
