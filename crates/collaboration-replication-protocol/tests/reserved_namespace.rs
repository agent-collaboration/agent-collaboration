#![allow(clippy::expect_used)]
use collaboration_replication_protocol::{
    SchemaAdmissionReason, SchemaEntry, SchemaObjectKind, admit_schema_statement,
    schema_fingerprint,
};

#[test]
fn every_reserved_group_is_excluded_before_schema_sql_is_parsed() {
    let empty = schema_fingerprint(&[]).expect("empty schema");
    for name in [
        "SQLITE_sequence",
        "TuRsO_cdc",
        "__TURSO_INTERNAL_seq",
        "_CF_KV",
        "__MINIFLARE_do_name",
        "_REPLICATION_state",
        "_COPY_state",
        "_SQLX_MIGRATIONS",
    ] {
        let entry = SchemaEntry {
            kind: SchemaObjectKind::Table,
            name: name.into(),
            sql: "platform-private SQL need not parse".into(),
        };
        assert_eq!(schema_fingerprint(&[entry]).expect(name), empty, "{name}");
        for sql in [
            format!("CREATE TABLE \"{name}\"(id INTEGER PRIMARY KEY)"),
            format!("CREATE VIEW ordinary AS SELECT * FROM main.\"{name}\""),
        ] {
            let error = admit_schema_statement(&sql).expect_err(&sql);
            assert!(
                matches!(error.reason, SchemaAdmissionReason::ProtectedObject { .. }),
                "{sql}"
            );
        }
    }
}

#[test]
fn similar_project_names_and_reserved_text_literals_remain_admitted() {
    for name in [
        "sqlitex_records",
        "turso",
        "_cf",
        "_copy",
        "_replication",
        "_sqlx_migrations_extra",
    ] {
        let sql = format!("CREATE TABLE \"{name}\"(id INTEGER PRIMARY KEY)");
        admit_schema_statement(&sql).expect(&sql);
        let entry = SchemaEntry {
            kind: SchemaObjectKind::Table,
            name: name.into(),
            sql,
        };
        assert_ne!(
            schema_fingerprint(&[entry]).expect(name),
            schema_fingerprint(&[]).expect("empty")
        );
    }
    admit_schema_statement("INSERT INTO records VALUES('__miniflare_do_name', '_replication_state', '_sqlx_migrations')").expect("text literals");
}
