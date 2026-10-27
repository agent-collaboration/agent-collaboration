#![expect(
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! A bookkeeping exclusion must not hide a board read, wherever SQL nests it.
#[path = "support/sql_cost_classification.rs"]
mod sql_cost_classification;
use sql_cost_classification::{DataStatementCounts, StatementTables};

#[test]
fn data_budget_counts_each_sql_kind_and_keeps_non_data_separate() {
    let mut data = DataStatementCounts::default();
    let other = data.count_sql("PRAGMA user_version; SELECT 1; INSERT INTO posts VALUES(1); UPDATE posts SET id=2; DELETE FROM posts; BEGIN; COMMIT;");
    assert_eq!(other, Some(3));
    assert_eq!(
        data,
        DataStatementCounts {
            selects: 1,
            inserts: 1,
            updates: 1,
            deletes: 1
        }
    );
    assert_eq!(data.total(), 4);
    assert_eq!(data.count_sql("SELECT * FROM"), None);
    assert_eq!(data.count_sql(""), None);
}

#[test]
fn diagnostic_kinds_never_expose_values_or_unrecognized_identifiers() {
    assert_eq!(
        StatementTables::diagnostic_kind("SELECT 'private-value' FROM posts"),
        "select [\"posts\"]"
    );
    assert_eq!(
        StatementTables::diagnostic_kind(
            "SELECT ? FROM subscription_memberships WHERE reader_actor='private-reader'"
        ),
        "select [\"subscription_memberships\"]"
    );
    assert_eq!(
        StatementTables::diagnostic_kind("BEGIN TRANSACTION private_name"),
        "begin []"
    );
    assert_eq!(
        StatementTables::diagnostic_kind("SELECT 'private-value' FROM private_table"),
        "select [\"other-table\"]"
    );
    assert_eq!(
        StatementTables::diagnostic_kind("private invalid SQL"),
        "unparsed"
    );
}

#[test]
fn only_library_tables_are_excluded_despite_quoting_literals_and_comments() {
    for statement in [
        "SELECT COUNT(*) FROM _fleet_sync_steps",
        "SELECT * FROM main.\"_fleet_sync_operations\" AS operations",
        "UPDATE `_fleet_sync_operations` SET next_at = ? WHERE id = ?",
        "DELETE FROM [_fleet_sync_steps] WHERE operation_id = ?",
        "INSERT INTO _fleet_sync_steps (id, value) VALUES (?, 'fleet_records')",
        "SELECT 'FROM fleet_records' FROM _fleet_sync_steps /* JOIN posts */",
    ] {
        assert!(
            StatementTables::parse(statement)
                .is_some_and(|tables| tables.only_library_bookkeeping()),
            "{statement}"
        );
    }
}

#[test]
fn every_mixed_or_board_query_remains_counted() {
    for statement in [
        "SELECT * FROM posts",
        "SELECT * FROM _fleet_sync_steps JOIN posts ON 1 = 1",
        "SELECT * FROM _fleet_sync_steps, posts",
        "SELECT * FROM _fleet_sync_steps WHERE EXISTS (SELECT 1 FROM posts)",
        "SELECT * FROM _fleet_sync_steps WHERE id IN posts",
        "SELECT * FROM _fleet_sync_steps UNION ALL SELECT * FROM posts",
        "WITH _fleet_sync_probe AS (SELECT * FROM posts) SELECT * FROM _fleet_sync_probe",
        "INSERT INTO _fleet_sync_steps SELECT * FROM posts",
        "UPDATE _fleet_sync_steps SET value = (SELECT value FROM posts)",
        "DELETE FROM _fleet_sync_steps WHERE id IN (SELECT id FROM posts)",
        "SELECT * FROM _fleet_sync_steps; SELECT * FROM posts",
    ] {
        assert!(
            !StatementTables::parse(statement)
                .is_some_and(|tables| tables.only_library_bookkeeping()),
            "{statement}"
        );
    }
}

#[test]
fn unknown_or_non_data_statements_remain_counted() {
    for statement in [
        "",
        "SELECT 1",
        "SELECT * FROM",
        "PRAGMA schema_version",
        "CREATE TABLE _fleet_sync_probe (id TEXT)",
        "BEGIN IMMEDIATE",
    ] {
        assert!(
            !StatementTables::parse(statement)
                .is_some_and(|tables| tables.only_library_bookkeeping()),
            "{statement}"
        );
    }
}
