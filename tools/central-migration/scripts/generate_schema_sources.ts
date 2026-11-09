// Writes src/generated/schema_sources.ts: the exact migration SQL the Worker
// needs, copied byte-for-byte from its sources so the Worker never retypes DDL.
//
// - Main's sets (b2a0d2280 = this checkout's crates/, plus the sync library at
//   the revision main's Cargo.lock pins) are what the Worker creates and
//   records, as a fresh bootstrap of main would (PD1).
// - Production's DDL (2636806a5 and library b9da4b5, through `git show` and
//   the library checkout) is what the Worker recognises, renames and copies.
//
// Run from tools/central-migration: `node scripts/generate_schema_sources.ts`.
// CENTRAL_MIGRATION_LIBRARY_MAIN_DIR=<agent-fleet-sync checkout> reads main's
// library sets from that checkout instead, for a local proof against a library
// revision not pinned yet; never commit a file generated that way.
// `test/schema_sources.test.ts` fails when the generated file is stale.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";

const REPOSITORY = resolve(import.meta.dirname, "../../..");
const PRODUCTION = "2636806a5";
const LIBRARY_CHECKOUTS = join(homedir(), ".cargo/git/checkouts/agent-fleet-sync-cc91c80fb7295419");
const LIBRARY_PRODUCTION = "b9da4b5";
const LIBRARY_MAIN = "0b27429";
const CORE = "crates/collaboration-central-core";

const mainFile = (path: string): string => readFileSync(join(REPOSITORY, CORE, path), "utf8");
const productionFile = (path: string): string =>
  execFileSync("git", ["-C", REPOSITORY, "show", `${PRODUCTION}:${CORE}/${path}`], {
    encoding: "utf8",
  });
const LIBRARY_MAIN_DIR =
  process.env.CENTRAL_MIGRATION_LIBRARY_MAIN_DIR ?? join(LIBRARY_CHECKOUTS, LIBRARY_MAIN);
const libraryDirectory = (revision: string): string =>
  revision === LIBRARY_MAIN ? LIBRARY_MAIN_DIR : join(LIBRARY_CHECKOUTS, revision);
const libraryFile = (revision: string, path: string): string =>
  readFileSync(join(libraryDirectory(revision), "crates", path), "utf8");

interface MigrationSource {
  readonly version: number;
  readonly description: string;
  readonly sql: string;
}

// Descriptions: central's sets as main's object_bootstrap.rs states them
// (lines 349–450); library sets as sqlx::migrate! derives them from the file
// name (underscores to spaces).
const centralFleet: MigrationSource[] = [
  [1, "fleet decisions", "core_migrations/0001_fleet_decisions.sql"],
  [2, "fleet decision receipts", "core_migrations/0002_fleet_decision_receipts.sql"],
  [3, "canonical fleet records", "core_migrations/0003_fleet_records.sql"],
  [4, "directory copy summary", "core_migrations/0004_directory_copy_summary.sql"],
  [5, "claim lease projection", "core_migrations/0005_claim_lease_projection.sql"],
  [6, "archive reports", "core_migrations/0006_archive_reports.sql"],
  [7, "immutable creation bindings", "core_migrations/0007_creation_decisions.sql"],
  [8, "Fleet feed readers", "core_migrations/0008_feed_readers.sql"],
  [9, "migration failure diagnostics", "core_migrations/0009_migration_failures.sql"],
  [10, "admin imports", "admin_import_migrations/0010_admin_imports.sql"],
].map(([version, description, path]) => ({
  version: version as number,
  description: description as string,
  sql: mainFile(path as string),
}));
const centralRegistry: MigrationSource[] = [
  [1, "machine enrollments", "registry_migrations/0001_machine_enrollments.sql"],
  [2, "lease incarnation", "registry_migrations/0002_lease_incarnation.sql"],
].map(([version, description, path]) => ({
  version: version as number,
  description: description as string,
  sql: mainFile(path as string),
}));
const libraryMigrations = (revision: string, folder: string, files: string[]) =>
  files.map((file) => {
    const match = /^(\d+)_(.+)\.sql$/.exec(file);
    if (match === null) throw new Error(`unexpected migration file ${file}`);
    return {
      version: Number(match[1]),
      description: (match[2] ?? "").replaceAll("_", " "),
      sql: libraryFile(revision, `${folder}/${file}`),
    };
  });
const LEASE_FILES = [
  "0001_registry_rules.sql",
  "0002_holder_workflow.sql",
  "0003_cleanup_failure.sql",
  "0004_schema_support.sql",
];
const RECEIVER_FILES = [
  "0001_copy_rules.sql",
  "0002_replicated_schema.sql",
  "0003_snapshot_install.sql",
];
const leaseMain = libraryMigrations(LIBRARY_MAIN, "fleet-sync-lease/migrations", LEASE_FILES);
const leaseProduction = libraryMigrations(
  LIBRARY_PRODUCTION,
  "fleet-sync-lease/migrations",
  LEASE_FILES,
);
const receiverMain = libraryMigrations(
  LIBRARY_MAIN,
  "fleet-sync-receiver/migrations",
  RECEIVER_FILES,
);
const receiverProduction = libraryMigrations(
  LIBRARY_PRODUCTION,
  "fleet-sync-receiver/migrations",
  RECEIVER_FILES,
);
for (const [main, production] of [
  [leaseMain, leaseProduction],
  [receiverMain, receiverProduction],
] as const) {
  main.forEach((migration, index) => {
    if (migration.sql !== production[index]?.sql) {
      throw new Error(`library migration ${migration.version} differs between production and main`);
    }
  });
}

const productionFleet = [
  "core_migrations/0000_migration_history.sql",
  "core_migrations/0001_fleet_decisions.sql",
  "core_migrations/0002_fleet_decision_receipts.sql",
  "core_migrations/0003_fleet_records.sql",
  "core_migrations/0004_directory_copy_summary.sql",
  "core_migrations/0005_claim_lease_projection.sql",
  "core_migrations/0006_archive_reports.sql",
  "core_migrations/0007_creation_decisions.sql",
  "core_migrations/0008_feed_readers.sql",
  "core_migrations/0009_migration_failures.sql",
  "admin_import_migrations/0010_admin_imports.sql",
].map(productionFile);
const productionRegistry = [
  "registry_migrations/0001_machine_enrollments.sql",
  "registry_migrations/0002_lease_incarnation.sql",
  "registry_migrations/0003_lease_acknowledgements.sql",
  "registry_migrations/0004_notice_marks.sql",
].map(productionFile);
const productionHistory = [
  productionFile("core_migrations/0000_migration_history.sql"),
  productionFile("core_migrations/0009_migration_failures.sql"),
];
// The retiring classes' storage, for the inventory's dump test seed.
const sessionProduction = ["202610160001_sessions.sql", "202610180001_attach_sequence.sql"].map(
  (file) => libraryFile(LIBRARY_PRODUCTION, `fleet-sync-session/migrations/${file}`),
);
const centralSessionProduction = [
  "session_migrations/0001_machine_notices.sql",
  "session_migrations/0002_forward_correlations.sql",
].map(productionFile);
const deliveryProduction = [
  "0001_delivery_custody.sql",
  "0002_mailbox_deadlines.sql",
  "0003_inbox_accounting.sql",
].map((file) => libraryFile(LIBRARY_PRODUCTION, `fleet-sync-delivery/migrations/${file}`));
// Main's journal corpus: every file sqlx::migrate! embeds from the folder, and
// the receipts table its Durable Object runner creates once the corpus records
// SQLx receipts (absent at libraries whose runner keeps _fleet_sync_schema).
const journalMain = libraryMigrations(
  LIBRARY_MAIN,
  "durable-steps/migrations",
  readdirSync(join(LIBRARY_MAIN_DIR, "crates/durable-steps/migrations"))
    .filter((file) => file.endsWith(".sql"))
    .sort(),
);
const receiptsPath = join(
  LIBRARY_MAIN_DIR,
  "crates/durable-steps/query-schema/99990000_journal_receipts.sql",
);
const journalReceiptsMain = existsSync(receiptsPath) ? readFileSync(receiptsPath, "utf8") : null;
const durableSteps = libraryFile(
  LIBRARY_PRODUCTION,
  "durable-steps/migrations/0001_durable_steps.sql",
);

const sources = {
  main: {
    centralFleet,
    centralRegistry,
    fleetSyncLease: leaseMain,
    fleetSyncReceiver: receiverMain,
    migrationFailures: mainFile("core_migrations/0009_migration_failures.sql"),
    journal: journalMain,
    journalReceipts: journalReceiptsMain,
  },
  production: {
    fleet: productionFleet,
    registry: productionRegistry,
    lease: leaseProduction.map((migration) => migration.sql),
    receiver: receiverProduction.map((migration) => migration.sql),
    history: productionHistory,
    // O session_runtime.rs:279–299: a Connection runs the session and delivery sets.
    connection: [...sessionProduction, ...centralSessionProduction, ...deliveryProduction],
    mailbox: deliveryProduction,
    // The Copy's consumer hook migration (O copy_receiver_rules.rs COPY_MIGRATIONS),
    // recorded in the library's _fleet_sync_hook_migrations, for the staging seed.
    copySummary: productionFile("copy_migrations/0001_copy_summary.sql"),
    journal: durableSteps,
  },
};

const output = `// GENERATED by scripts/generate_schema_sources.ts from:
// - main ${CORE} (this checkout, b2a0d2280) and agent-fleet-sync ${LIBRARY_MAIN};
// - production ${PRODUCTION} and agent-fleet-sync ${LIBRARY_PRODUCTION}.
// Do not edit; regenerate. test/schema_sources.test.ts checks it is current.

export const SCHEMA_SOURCES = ${JSON.stringify(sources, null, 2)} as const;
`;
const target = resolve(import.meta.dirname, "../src/generated/schema_sources.ts");
if (process.argv.includes("--check")) {
  if (readFileSync(target, "utf8") !== output) {
    console.error("src/generated/schema_sources.ts is stale; regenerate it");
    process.exit(1);
  }
} else {
  writeFileSync(target, output);
}
