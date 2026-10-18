#![allow(clippy::expect_used)]
use collaboration_replication_protocol::*;
#[test]
fn each_admitted_form_preserves_normal_project_migrations() {
    for sql in [
        "CREATE TABLE records(id INTEGER PRIMARY KEY)",
        "CREATE INDEX record_idx ON records(id)",
        "CREATE VIEW record_view AS SELECT id FROM records",
        "ALTER TABLE records ADD COLUMN label TEXT",
        "DROP TABLE records",
        "DROP INDEX record_idx",
        "DROP VIEW record_view",
        "INSERT INTO records VALUES(1)",
        "UPDATE records SET id=2 WHERE id=1",
        "DELETE FROM records WHERE id=1",
        "WITH keep AS (SELECT id FROM records) INSERT INTO records SELECT id FROM keep",
        "INSERT INTO records VALUES('_copy_state')",
    ] {
        assert!(admit_schema_statement(sql).is_ok(), "{sql}");
    }
}
#[test]
fn every_unadmitted_statement_form_is_refused() {
    for (sql, form) in [
        (
            "CREATE TRIGGER tr AFTER INSERT ON records BEGIN UPDATE records SET id=1; END",
            SchemaStatementForm::CreateTrigger,
        ),
        ("DROP TRIGGER tr", SchemaStatementForm::DropTrigger),
        (
            "CREATE VIRTUAL TABLE records USING fts5(label)",
            SchemaStatementForm::VirtualTable,
        ),
        ("ATTACH 'other.db' AS other", SchemaStatementForm::Attach),
        ("DETACH other", SchemaStatementForm::Detach),
        ("PRAGMA user_version=1", SchemaStatementForm::Pragma),
        ("VACUUM", SchemaStatementForm::Vacuum),
        ("REINDEX records", SchemaStatementForm::Reindex),
        ("ANALYZE records", SchemaStatementForm::Analyze),
        ("BEGIN IMMEDIATE", SchemaStatementForm::TransactionControl),
        ("COMMIT", SchemaStatementForm::TransactionControl),
        ("ROLLBACK", SchemaStatementForm::TransactionControl),
        ("SAVEPOINT s", SchemaStatementForm::TransactionControl),
        ("RELEASE s", SchemaStatementForm::TransactionControl),
        ("SELECT id FROM records", SchemaStatementForm::Select),
        (
            "EXPLAIN SELECT id FROM records",
            SchemaStatementForm::Explain,
        ),
    ] {
        assert_eq!(
            admit_schema_statement(sql).expect_err(sql).form,
            form,
            "{sql}"
        );
    }
}
#[test]
fn protected_objects_are_detected_in_targets_joins_subqueries_and_ctes() {
    for sql in [
        "CREATE TABLE '_copy_quoted'(id INTEGER PRIMARY KEY)",
        "CREATE TABLE [_copy_bracketed](id INTEGER PRIMARY KEY)",
        "CREATE TABLE safe(id INTEGER PRIMARY KEY REFERENCES _COPY_state(singleton))",
        "CREATE INDEX allowed_index ON _copy_state(singleton)",
        "CREATE VIEW safe_view AS WITH hidden AS (SELECT * FROM _copy_state) SELECT * FROM hidden",
        "ALTER TABLE main._COPY_state ADD COLUMN counterfeit TEXT",
        "DROP TABLE _copy_state",
        "DROP INDEX sqlite_autoindex_state",
        "DROP VIEW _Cf_hidden",
        "UPDATE records SET id=1 FROM _copy_state WHERE id=singleton",
        "INSERT INTO records SELECT id FROM records WHERE id IN _copy_state",
        "DELETE FROM main._copy_state WHERE singleton=1",
        "CREATE TABLE _COPY_forged(id INTEGER PRIMARY KEY)",
        "INSERT INTO records SELECT id FROM sqlite_schema",
        "UPDATE records SET id=(SELECT id FROM _Cf_Metadata)",
        "DELETE FROM records WHERE EXISTS(SELECT 1 FROM turso_cdc)",
        "CREATE VIEW v AS SELECT id FROM records JOIN __TURSO_INTERNAL_seq ON 1=1",
        "WITH safe AS (SELECT id FROM records), hidden AS (SELECT id FROM main.\"_cOpY_state\") INSERT INTO records SELECT id FROM hidden",
        "WITH _copy_alias AS (SELECT id FROM records) INSERT INTO records SELECT id FROM _copy_alias",
        "ALTER TABLE records RENAME TO \"_Copy_hidden\"",
        "CREATE INDEX _copy_idx ON records(id)",
        "INSERT INTO records SELECT id FROM records WHERE id IN(SELECT id FROM (SELECT id FROM `_copy_state`))",
    ] {
        assert!(
            matches!(
                admit_schema_statement(sql),
                Err(SchemaAdmissionError {
                    reason: SchemaAdmissionReason::ProtectedObject { .. },
                    ..
                })
            ),
            "{sql}"
        );
    }
}
#[test]
fn empty_invalid_and_multi_statement_input_is_refused_without_a_fallback() {
    for sql in ["", "not valid SQL", "CREATE TABLE a(id); DROP TABLE b"] {
        assert!(admit_schema_statement(sql).is_err(), "{sql}");
    }
}
#[test]
fn schema_unsupported_refusal_round_trips_as_a_typed_form() {
    let refusal = SessionRefusal::SchemaUnsupported {
        form: SchemaStatementForm::Insert,
    };
    let wire = serde_json::json!({"kind":"schemaUnsupported","form":"insert"});
    assert_eq!(serde_json::to_value(&refusal).expect("wire"), wire);
    assert_eq!(
        serde_json::from_value::<SessionRefusal>(wire).expect("decode"),
        refusal
    );
}
