use crate::schema_fingerprint::fingerprint_entries;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[test]
fn reserved_names_are_excluded_case_insensitively_including_local_platform_tables() -> TestResult {
    let empty = fingerprint_entries(Vec::new())?;
    for name in [
        "SQLITE_Sequence",
        "TURSO_changes",
        "__TURSO_INTERNAL_meta",
        "_CF_KV",
        "__MINIFLARE_do_name",
        "_Replication_State",
        "_Copy_State",
        "_SQLX_MIGRATIONS",
    ] {
        let identity = fingerprint_entries(vec![(
            "table".into(),
            name.into(),
            format!("CREATE TABLE \"{name}\"(id INTEGER PRIMARY KEY)"),
        )])?;
        assert_eq!(identity, empty, "reserved table {name}");
    }
    for name in [
        "replication_state",
        "_sqlx_migrations_application",
        "__miniflareapplication",
    ] {
        assert_ne!(
            fingerprint_entries(vec![(
                "table".into(),
                name.into(),
                format!("CREATE TABLE \"{name}\"(id INTEGER PRIMARY KEY)")
            )])?,
            empty,
            "application table {name} must remain visible"
        );
    }
    Ok(())
}

#[test]
fn identifier_quotes_and_punctuation_spacing_do_not_change_schema_identity() -> TestResult {
    let first = fingerprint_entries(vec![(
        "table".into(),
        "child".into(),
        "CREATE TABLE child(a TEXT,b TEXT,PRIMARY KEY(a,b),FOREIGN KEY(a) REFERENCES parent(id))"
            .into(),
    )])?;
    let second = fingerprint_entries(vec![("table".into(), "child".into(), "CREATE TABLE \"child\" (\"a\" TEXT, \"b\" TEXT, PRIMARY KEY (\"a\", \"b\"), FOREIGN KEY (\"a\") REFERENCES \"parent\" (\"id\"))".into())])?;
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn string_literal_whitespace_and_constraint_changes_remain_distinct() -> TestResult {
    let identity =
        |sql: &str| fingerprint_entries(vec![("table".into(), "items".into(), sql.into())]);
    assert_ne!(
        identity("CREATE TABLE items(id INTEGER PRIMARY KEY,value TEXT DEFAULT 'a  (b), c')")?,
        identity("CREATE TABLE items(id INTEGER PRIMARY KEY,value TEXT DEFAULT 'a (b), c')")?
    );
    assert_ne!(
        identity("CREATE TABLE items(id INTEGER PRIMARY KEY,value BOOLEAN CHECK(value IN (0,1)))")?,
        identity("CREATE TABLE items(id INTEGER PRIMARY KEY,value BOOLEAN CHECK(value IN (0,2)))")?
    );
    Ok(())
}

#[test]
fn bookkeeping_exclusion_is_exact_for_sqlx_history() -> TestResult {
    let empty = fingerprint_entries(Vec::new())?;
    assert_eq!(
        fingerprint_entries(vec![(
            "table".into(),
            "_sqlx_migrations".into(),
            "CREATE TABLE _sqlx_migrations(version INTEGER PRIMARY KEY)".into()
        )])?,
        empty
    );
    assert_ne!(
        fingerprint_entries(vec![(
            "table".into(),
            "_sqlx_application".into(),
            "CREATE TABLE _sqlx_application(id INTEGER PRIMARY KEY)".into()
        )])?,
        empty
    );
    Ok(())
}
