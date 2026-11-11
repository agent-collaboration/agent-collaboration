//! The staging seed Worker's table-digest fixture, computed by the Rust side.
//!
//! `verification/central-seed-worker/test/fixtures/table-digest-cases.json`
//! records each case's digest. This test recomputes every case with the sync
//! library's own `TableDigestBuilder` (byte-identical at fleet-sync b9da4b5 and
//! 0b2742975) and fails unless the recorded digest matches, so the Worker's
//! TypeScript port is checked against Rust, not against itself. It also
//! records the schema fingerprint of the seed test's schema, which old central
//! must report for the seeded Copy.
use fleet_sync_protocol::{SchemaEntry, SchemaObjectKind, SqlValue, TableDigestBuilder};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
    schema: Schema,
}

#[derive(Deserialize)]
struct Schema {
    entries: Vec<Entry>,
    fingerprint: String,
}

#[derive(Deserialize)]
struct Entry {
    kind: String,
    name: String,
    sql: String,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    rows: Vec<Vec<SqlValue>>,
    digest: String,
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The assertions are the check; fixture errors propagate through Result."
)]
#[test]
fn every_seed_digest_case_matches_the_library_table_digest()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../../verification/central-seed-worker/test/fixtures/table-digest-cases.json"
    ))?;
    assert!(!fixture.cases.is_empty());
    let entries = fixture
        .schema
        .entries
        .into_iter()
        .map(|entry| {
            let kind = match entry.kind.as_str() {
                "table" => SchemaObjectKind::Table,
                "index" => SchemaObjectKind::Index,
                "view" => SchemaObjectKind::View,
                "trigger" => SchemaObjectKind::Trigger,
                other => return Err(format!("unknown schema kind {other}")),
            };
            Ok(SchemaEntry {
                kind,
                name: entry.name,
                sql: entry.sql,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        fleet_sync_protocol::schema_fingerprint(&entries)?,
        fixture.schema.fingerprint,
        "recorded schema fingerprint differs from schema_fingerprint"
    );
    for case in fixture.cases {
        let mut digest = TableDigestBuilder::new();
        for row in &case.rows {
            digest.push_row(row)?;
        }
        let computed = serde_json::to_value(digest.finish())?;
        assert_eq!(
            computed,
            serde_json::Value::String(case.digest.clone()),
            "{}: recorded digest differs from TableDigestBuilder",
            case.name
        );
    }
    Ok(())
}
