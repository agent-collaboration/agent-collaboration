//! Every table a candidate object may hold, and how the gate covers it. A
//! table missing here is refused, so a new table family can't pass unseen.
//!
//! - `Decoded`: the gate decodes its rows with main's own decoder.
//! - `DecodedWith`: main decodes it as part of another table's record, which the
//!   gate decodes (a workspace's repository refs, an ADR's files, …).
//! - `MustBeEmpty`: main decodes it, the migration carries nothing into it, and
//!   the gate has no reason to: any row is refused until the gate decodes it.
//! - `NotReadByMain`: kept for revert, or a ledger main's bootstrap verifies
//!   (proven separately by main-central opening the migrated objects).

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Coverage {
    Decoded(&'static str),
    DecodedWith(&'static str),
    MustBeEmpty(&'static str),
    NotReadByMain(&'static str),
}

const LEASE_ROWS: Coverage = Coverage::Decoded("fleet-sync-lease legacy-row validation");
const RETIRED: Coverage = Coverage::NotReadByMain("the old table, kept for revert");
const LEDGER: Coverage = Coverage::NotReadByMain(
    "a migration ledger main's bootstrap verifies (main-central proves it)",
);
const JOURNAL: Coverage = Coverage::NotReadByMain(
    "the durable-steps journal main's library adopts (main-central proves it)",
);
const RETAINED: Coverage = Coverage::Decoded(
    "inspect_retained_records: the receiver's own reader, its count equal to the stored rows",
);

pub const FLEET: &[(&str, Coverage)] = &[
    (
        "fleet_workspaces",
        Coverage::Decoded("canonical workspace record"),
    ),
    (
        "fleet_workspace_repository_refs",
        Coverage::DecodedWith("fleet_workspaces"),
    ),
    ("fleet_boards", Coverage::Decoded("canonical board record")),
    (
        "fleet_workspace_adrs",
        Coverage::Decoded("canonical ADR record"),
    ),
    (
        "fleet_adr_repository_files",
        Coverage::DecodedWith("fleet_workspace_adrs"),
    ),
    (
        "fleet_project_directory",
        Coverage::Decoded("directory record, Copy summary and lease projection"),
    ),
    (
        "fleet_repository_attachments",
        Coverage::Decoded("repository attachments per workspace"),
    ),
    (
        "fleet_request_decisions",
        Coverage::Decoded("request ledger record"),
    ),
    (
        "fleet_decision_reasons",
        Coverage::DecodedWith("fleet_request_decisions"),
    ),
    (
        "fleet_decision_reason_evidence",
        Coverage::DecodedWith("fleet_request_decisions"),
    ),
    (
        "fleet_decision_receipts",
        Coverage::DecodedWith("fleet_request_decisions"),
    ),
    (
        "fleet_feed_state",
        Coverage::Decoded("feed state and retained range"),
    ),
    ("fleet_feed", Coverage::DecodedWith("fleet_feed_state")),
    (
        "fleet_feed_repository_refs",
        Coverage::DecodedWith("fleet_feed_state"),
    ),
    (
        "fleet_feed_adr_repository_files",
        Coverage::DecodedWith("fleet_feed_state"),
    ),
    (
        "machine_enrollments",
        Coverage::Decoded("StoredMachine::decode per machine"),
    ),
    (
        "lease_incarnation",
        Coverage::Decoded("lease incarnation counter"),
    ),
    (
        "lease_claim_reservations",
        Coverage::MustBeEmpty("claim reservation"),
    ),
    (
        "lease_claim_settlement_receipts",
        Coverage::MustBeEmpty("claim settlement receipt"),
    ),
    (
        "fleet_archive_reports",
        Coverage::MustBeEmpty("archive report"),
    ),
    (
        "fleet_creation_decisions",
        Coverage::MustBeEmpty("creation decision"),
    ),
    ("fleet_feed_readers", Coverage::MustBeEmpty("feed reader")),
    ("fleet_admin_imports", Coverage::MustBeEmpty("admin import")),
    (
        "fleet_admin_import_manifest_tables",
        Coverage::MustBeEmpty("admin import manifest"),
    ),
    (
        "central_schema_migration_failures",
        Coverage::MustBeEmpty("migration failure (R7)"),
    ),
    ("_fleet_sync_databases", LEASE_ROWS),
    ("_fleet_sync_leases", LEASE_ROWS),
    ("_fleet_sync_epoch_floor", LEASE_ROWS),
    ("_fleet_sync_renewal_attempts", LEASE_ROWS),
    ("_fleet_sync_epoch_history", LEASE_ROWS),
    ("_fleet_sync_transfers", LEASE_ROWS),
    ("_fleet_sync_takeover_previews", LEASE_ROWS),
    ("_fleet_sync_lifecycle_attempts", LEASE_ROWS),
    ("_fleet_sync_transfer_admissions", LEASE_ROWS),
    ("_fleet_sync_transfer_cancels", LEASE_ROWS),
    ("_fleet_sync_schema_support", LEASE_ROWS),
    ("_sqlx_migrations_central_fleet", LEDGER),
    ("_sqlx_migrations_central_registry", LEDGER),
    ("_sqlx_migrations_fleet_sync_lease", LEDGER),
    ("_fleet_sync_operations", JOURNAL),
    ("_fleet_sync_steps", JOURNAL),
    ("_fleet_sync_migrations", JOURNAL),
    ("central_schema_migrations", RETIRED),
    ("workspaces", RETIRED),
    ("boards", RETIRED),
    ("project_directory", RETIRED),
    ("workspace_adrs", RETIRED),
    ("repository_attachments", RETIRED),
    ("claim_reservations", RETIRED),
    ("claim_settlement_receipts", RETIRED),
    ("archive_reports", RETIRED),
    ("creation_decisions", RETIRED),
    ("feed_readers", RETIRED),
    ("admin_imports", RETIRED),
    ("lease_incarnation_fence", RETIRED),
    ("legacy_fleet_decision_receipts", RETIRED),
    ("legacy_fleet_feed", RETIRED),
    ("legacy_fleet_feed_state", RETIRED),
    ("legacy_fleet_request_decisions", RETIRED),
    ("legacy_central_schema_migration_failures", RETIRED),
    ("legacy_journal_operations", RETIRED),
    ("legacy_journal_steps", RETIRED),
];

pub const COPY: &[(&str, Coverage)] = &[
    (
        "_copy_summary",
        Coverage::Decoded("ReadCopySummary's row rules and public CopyDirectorySummary decoder"),
    ),
    (
        "_fleet_sync_copy",
        Coverage::Decoded("copy_status and inspect_copy, after main's receiver migration"),
    ),
    (
        "_fleet_sync_staged_parts",
        Coverage::Decoded("inspect_copy staged parts, and each payload as apply reads it"),
    ),
    (
        "_fleet_sync_schema_steps",
        Coverage::Decoded("inspect_copy schema fingerprint"),
    ),
    (
        "_fleet_sync_receipts",
        Coverage::Decoded("receipt digest (PieceDigest)"),
    ),
    (
        "_fleet_sync_copy_tables",
        Coverage::Decoded("replicated schema membership (TableName)"),
    ),
    ("_fleet_sync_fence_receipts", RETAINED),
    ("_fleet_sync_seed", RETAINED),
    ("_fleet_sync_seed_pages", RETAINED),
    ("_fleet_sync_seed_attempts", RETAINED),
    ("_fleet_sync_creation", RETAINED),
    ("_fleet_sync_install", RETAINED),
    ("_fleet_sync_install_pages", RETAINED),
    ("_fleet_sync_hook_migrations", LEDGER),
    ("_sqlx_migrations_fleet_sync_receiver", LEDGER),
    (
        "central_schema_migration_failures",
        Coverage::MustBeEmpty("migration failure (R7)"),
    ),
    ("_fleet_sync_operations", JOURNAL),
    ("_fleet_sync_steps", JOURNAL),
    ("_fleet_sync_migrations", JOURNAL),
    ("central_schema_migrations", RETIRED),
    ("legacy_central_schema_migration_failures", RETIRED),
    ("legacy_journal_operations", RETIRED),
    ("legacy_journal_steps", RETIRED),
];

pub fn coverage(map: &[(&str, Coverage)], table: &str) -> Option<Coverage> {
    map.iter()
        .find(|(name, _)| *name == table)
        .map(|(_, coverage)| *coverage)
}
