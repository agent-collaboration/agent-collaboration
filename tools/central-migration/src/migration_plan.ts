// Inventory and migrate share one pure decision: read every old record, check
// it (R7), and either stop with every contradiction named, or produce each
// object's single transaction of statements. Nothing here touches storage.

import { enrollmentRow } from "./enrollment_record.ts";
import { I64_MAX, exactUnsigned, parseExact } from "./exact_json.ts";
import { Rejected, convertFleetGraph, insertStatement } from "./fleet_conversion.ts";
import { Invalid } from "./serde_shapes.ts";
import { checkLeaseTables, decodeCopyRecord, type CopyRecord } from "./lease_records.ts";
import type { MacFileEvidence } from "./mac_file_evidence.ts";
import {
  compareMacFile,
  configuredReceiverOf,
  copyReportOf,
  type CopyReport,
} from "./holder_file_checks.ts";
import {
  checkHistory,
  checkJournal,
  checkShape,
  receiverChecksumsMatch,
  rows,
  type Stop,
} from "./storage_checks.ts";
import type { CarriedFleetRows, NewRow } from "./fleet_conversion.ts";
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

export type { CopyReport, MacFileEvidence, Stop };

export interface ProjectReport {
  readonly project: string;
  readonly holder: string | null;
  readonly leaseNumber: bigint | null;
  readonly lifecycle: string;
  readonly directory: {
    readonly workspace: string;
    readonly board: string;
    readonly name: string;
  } | null;
  readonly copy: CopyReport | null;
  readonly action: "carry" | "stop";
}

export interface MigrationPlan {
  readonly runTime: number;
  /** Old central's authoritative receiver (its CENTRAL_REGISTRY_CONFIG), or null when unreadable. */
  readonly configuredReceiver: string | null;
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

function json(value: unknown): Record<string, unknown> | null {
  if (typeof value !== "string") return null;
  try {
    const parsed: unknown = parseExact(value);
    return typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
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

export interface PlanOptions {
  /** One recorded instant, whole seconds, for every created_at, updated_at and installed_on written. */
  readonly runTime: number;
  /** Old central's CENTRAL_REGISTRY_CONFIG text, as deployed with the migration Worker. */
  readonly registryConfig: string | undefined;
  /** S2's Mac-file facts; null for inventory, which reports central's side only. */
  readonly macFiles: readonly MacFileEvidence[] | null;
}

/** The whole decision: every R7 stop, or each object's single transaction. */
export async function planMigration(old: OldStorage, options: PlanOptions): Promise<MigrationPlan> {
  const { runTime } = options;
  const stops: Stop[] = [];
  const runTimeText = new Date(runTime).toISOString();
  const installedOn = runTimeText.replace("T", " ").slice(0, 19);
  const { registry, fleet } = old;
  const configuredReceiver = configuredReceiverOf(options.registryConfig);
  if (configuredReceiver === null) {
    stops.push({
      object: "central config",
      subject: "authoritative receiver",
      reason: "CENTRAL_REGISTRY_CONFIG names no canonical UUIDv7 authoritativeReceiver",
    });
  }

  checkShape("registry", registry, PRODUCTION_TABLES.registry, true, stops);
  checkShape("fleet", fleet, PRODUCTION_TABLES.fleet, true, stops);
  checkHistory("registry", registry, PRODUCTION_SETS.registry, stops);
  checkHistory("fleet", fleet, PRODUCTION_SETS.fleet, stops);
  checkJournal("registry", registry, stops);
  checkJournal("fleet", fleet, stops);
  // Every carried lease-table row, as main's first open decodes it (R7, I1).
  for (const stop of checkLeaseTables(registry)) {
    stops.push({ object: "registry", subject: `${stop.table} ${stop.row}`, reason: stop.reason });
  }

  // Machines (PD3).
  const machines = new Map<string, NewRow>();
  for (const row of rows(registry, "machine_enrollments")) {
    const converted = enrollmentRow(row, runTimeText);
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
  const macFiles = new Map((options.macFiles ?? []).map((file) => [file.database, file]));

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
    const leaseNumber = exactUnsigned(lease?.epoch);
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
      const storedCopy = rows(copyReply, "_fleet_sync_copy")[0]?.record;
      let record: CopyRecord | null = null;
      try {
        record = decodeCopyRecord(storedCopy);
      } catch (error) {
        if (!(error instanceof Invalid)) throw error;
        stops.push({
          object: `copy ${project}`,
          subject,
          reason: `main refuses the copy record: ${error.message}`,
        });
      }
      copy =
        record === null
          ? {
              state: "unreadable",
              role: null,
              fence: null,
              holder: null,
              start: null,
              acked: null,
              schema: null,
            }
          : copyReportOf(record);
      const { state, fence } = copy;
      const copyHolder = copy.holder;
      if (record !== null && record.state === "retired" && record.database !== project) {
        stops.push({
          object: `copy ${project}`,
          subject,
          reason: `retired copy names ${record.database}`,
        });
      }
      if (record !== null && record.state !== "retired" && record.status.database !== project) {
        stops.push({
          object: `copy ${project}`,
          subject,
          reason: `copy names ${record.status.database}`,
        });
      }
      // A retired copy is preserved only beside a retired registry record (PD5, 81d442e3).
      if (record !== null && (state === "retired") !== (lifecycle === "retired")) {
        stops.push({
          object: `copy ${project}`,
          subject,
          reason: `copy is ${state} but the registry lifecycle is ${lifecycle}`,
        });
      }
      if (holder !== null && record !== null) {
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
        } else if (options.macFiles !== null) {
          const mismatch = compareMacFile(
            project,
            holder,
            leaseNumber,
            copy,
            configuredReceiver,
            macFiles.get(project),
          );
          if (mismatch !== null)
            stops.push({ object: `copy ${project}`, subject, reason: mismatch });
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
  for (const project of macFiles.keys()) {
    if (!projects.some((report) => report.project === project && report.holder !== null)) {
      stops.push({
        object: "mac files",
        subject: `project ${project}`,
        reason: "retained-file facts name a project central holds no lease for",
      });
    }
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
  const lastPosition = exactUnsigned(feedState[0]?.last_position);
  if (feedState.length === 1 && (lastPosition === null || lastPosition > I64_MAX)) {
    stops.push({
      object: "fleet",
      subject: "feed state",
      reason: "the feed head is not an integer main can store",
    });
  }
  // Lease incarnation: the exact larger of production's two counters, each a
  // canonical decimal main parses (fleet_lease_projection_step.rs) and an i64 it can store.
  const incarnations: bigint[] = [];
  for (const [object, table] of [
    ["registry", "registry_lease_incarnation"],
    ["fleet", "lease_incarnation_fence"],
  ] as const) {
    for (const row of rows(object === "registry" ? registry : fleet, table)) {
      const counter = typeof row.incarnation === "string" ? exactUnsigned(row.incarnation) : null;
      if (counter === null || counter > I64_MAX) {
        stops.push({
          object,
          subject: `${table} ${String(row.name)}`,
          reason: `incarnation ${String(row.incarnation)} is not a canonical decimal within i64`,
        });
      } else {
        incarnations.push(counter);
      }
    }
  }

  const counts: Record<string, number> = {
    projects: projects.length,
    machines: machines.size,
    ...(graph === null
      ? {}
      : Object.fromEntries(TABLE_ORDER.map((table) => [table, graph[table].length]))),
  };
  if (stops.length > 0 || graph === null) {
    return { runTime, configuredReceiver, stops, projects, counts, fleet: [], copies: {} };
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
  // Head and floor both at the old head (PD4): a Router behind it sees an
  // expired cursor and rebuilds; one at it reads an empty page.
  statements.push(
    insertStatement("fleet_feed_state", {
      state_id: String(head?.state_id),
      name: String(head?.name),
      last_position: lastPosition ?? 0n,
      retained_through: lastPosition ?? 0n,
      created_at: runTime,
      updated_at: runTime,
    }),
  );
  for (const machine of machines.values())
    statements.push(insertStatement("machine_enrollments", machine));
  if (incarnations.length > 0) {
    statements.push(
      insertStatement("lease_incarnation", {
        row_id: uuidV7(runTime),
        name: "fleet",
        incarnation: incarnations.reduce((larger, counter) =>
          counter > larger ? counter : larger,
        ),
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
  return { runTime, configuredReceiver, stops, projects, counts, fleet: statements, copies };
}
