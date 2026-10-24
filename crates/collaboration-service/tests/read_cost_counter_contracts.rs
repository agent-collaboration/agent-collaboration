//! A bookkeeping exclusion must not hide a board read, wherever SQL nests it.
#[path = "support/sql_cost_classification.rs"]
mod sql_cost_classification;
use sql_cost_classification::StatementTables;

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
