#![allow(clippy::expect_used)]

use collaboration_replication_protocol::{SchemaEntry, SchemaObjectKind, schema_fingerprint};

fn table(name: &str, sql: &str) -> SchemaEntry {
    SchemaEntry {
        kind: SchemaObjectKind::Table,
        name: name.into(),
        sql: sql.into(),
    }
}

#[test]
fn fingerprint_sorts_entries_and_normalizes_sql_whitespace() {
    let entries = vec![
        table("b", "  CREATE\nTABLE\t b(id INTEGER PRIMARY KEY)  "),
        table("a", "CREATE TABLE a(id INTEGER PRIMARY KEY)"),
    ];
    let reordered = vec![
        table("a", "CREATE TABLE a(id INTEGER PRIMARY KEY)"),
        table("b", "CREATE TABLE b(id INTEGER PRIMARY KEY)"),
    ];
    assert_eq!(
        schema_fingerprint(&entries).expect("fingerprint"),
        schema_fingerprint(&reordered).expect("fingerprint")
    );
}

#[test]
fn fingerprint_excludes_engine_and_both_sides_bookkeeping_entries() {
    let entries = [table(
        "records",
        "CREATE TABLE records(id INTEGER PRIMARY KEY)",
    )];
    let expected = schema_fingerprint(&entries).expect("fingerprint");
    let mut with_internals = entries.to_vec();
    for name in [
        "sqlite_sequence",
        "turso_cdc",
        "turso_metadata",
        "__turso_internal_seq___turso_internal_autoincrement_turso_cdc",
        "_cf_METADATA",
        "_replication_state",
        "_copy_state",
        "_sqlx_migrations",
    ] {
        with_internals.push(table(
            name,
            &format!("CREATE TABLE {name}(id INTEGER PRIMARY KEY)"),
        ));
    }
    assert_eq!(
        schema_fingerprint(&with_internals).expect("fingerprint"),
        expected
    );
}

#[test]
fn fingerprint_changes_with_schema_kind_name_or_sql() {
    let baseline = [table(
        "records",
        "CREATE TABLE records(id INTEGER PRIMARY KEY)",
    )];
    let expected = schema_fingerprint(&baseline).expect("fingerprint");
    for entry in [
        table(
            "other_records",
            "CREATE TABLE records(id INTEGER PRIMARY KEY)",
        ),
        table(
            "records",
            "CREATE TABLE records(id INTEGER PRIMARY KEY, text TEXT)",
        ),
        SchemaEntry {
            kind: SchemaObjectKind::View,
            name: "records".into(),
            sql: baseline.first().expect("entry").sql.clone(),
        },
    ] {
        assert_ne!(schema_fingerprint(&[entry]).expect("fingerprint"), expected);
    }
}

#[test]
fn empty_schema_matches_independent_sha256_of_empty_json_list() {
    assert_eq!(
        schema_fingerprint(&[]).expect("fingerprint"),
        "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945"
    );
    assert!(SchemaObjectKind::try_from("unknown").is_err());
}

#[test]
fn fingerprint_canonicalizes_ddl_spacing_and_identifier_quoting_across_engines() {
    for (left, right) in [
        (
            "CREATE TABLE t(a INTEGER PRIMARY KEY)",
            "CREATE TABLE \"t\" (\"a\" INTEGER PRIMARY KEY)",
        ),
        (
            "CREATE TABLE t(a TEXT DEFAULT 'two  spaces')",
            "create table `t` ( [a] TEXT DEFAULT 'two  spaces')",
        ),
        (
            "CREATE INDEX ix ON t(a) WHERE a IS NOT NULL",
            "CREATE INDEX \"ix\" ON \"t\" (\"a\") WHERE \"a\" IS NOT NULL",
        ),
    ] {
        assert_eq!(
            schema_fingerprint(&[table("t", left)]).expect("left"),
            schema_fingerprint(&[table("t", right)]).expect("right")
        );
    }
}
#[test]
fn fingerprint_preserves_string_literals_and_rejects_invalid_or_multiple_commands() {
    assert_ne!(
        schema_fingerprint(&[table("t", "CREATE TABLE t(a TEXT DEFAULT 'two spaces')")])
            .expect("left"),
        schema_fingerprint(&[table("t", "CREATE TABLE t(a TEXT DEFAULT 'two  spaces')")])
            .expect("right")
    );
    for sql in [
        "",
        "not valid SQL",
        "CREATE TABLE a(id); CREATE TABLE b(id)",
    ] {
        assert!(schema_fingerprint(&[table("t", sql)]).is_err());
    }
}
