//! The two table families present in the real staging inventory but absent
//! from the original fixture: published summaries and retired Board tables.

use super::{edited, text};
use crate::gate::{Outcome, run_gate};
use board_fleet_sync::BoardProjectSchema;
use fleet_sync_writer::ReplicatedSchema;
use serde_json::{Value, json};
use sha2::{Digest, Sha384};

const LIVE: &str = "01a1c070-0000-7000-8000-000000000001";
const RETIRED: &str = "01a1c070-0000-7000-8000-000000000003";
const SUMMARY_SQL: &str =
    include_str!("../../../collaboration-central-core/copy_migrations/0001_copy_summary.sql");

fn summary() -> Value {
    json!({
        "database": LIVE,
        "summary": {"kind": "projectSummary", "value": {"latestRecordAt": {"kind": "legacyUnknown"}}},
        "position": {"epoch": 3, "sequence": 17},
        "ackedAt": 1794312000000_i64
    })
}

fn with_summary(candidate: &mut Value) {
    let copy = &mut candidate["copies"][LIVE];
    copy["schema"].as_array_mut().expect("schema").push(json!({
        "type": "table", "name": "_copy_summary", "sql": SUMMARY_SQL
    }));
    copy["tables"]["_copy_summary"] = json!({
        "columns": ["database_id", "record"],
        "rows": [[text(LIVE), text(&summary().to_string())]]
    });
    // The old receiver recorded the summary hook's actual SQL checksum.
    let checksum = serde_json::to_string(Sha384::digest(SUMMARY_SQL.as_bytes()).as_slice())
        .expect("checksum serializes");
    copy["tables"]["_fleet_sync_hook_migrations"]["rows"] = json!([[text("1"), text(&checksum)]]);
}

#[tokio::test]
async fn a_published_copy_summary_is_decoded_and_accepted() {
    // Arrange / Act
    let report = run_gate(&edited(with_summary)).await;

    // Assert: the old gate refused this real table as uncovered.
    assert_eq!(report.outcome, Outcome::Pass, "{report:?}");
}

#[tokio::test]
async fn copy_summary_json_values_must_not_normalize_but_formatting_may_differ() {
    // Arrange: main's decode_json compares JSON values, not stored bytes.
    let mut offset = summary();
    offset["summary"]["value"]["latestRecordAt"] =
        json!({"kind": "router", "value": "2026-11-10T13:00:00+01:00"});
    let decoded: collaboration_central_core::CopyDirectorySummary =
        serde_json::from_value(offset.clone()).expect("Serde accepts the offset time");
    assert_ne!(serde_json::to_value(decoded).expect("serializes"), offset);
    let inventory = edited(|candidate| {
        with_summary(candidate);
        candidate["copies"][LIVE]["tables"]["_copy_summary"]["rows"][0][1] =
            text(&offset.to_string());
    });
    let formatted = edited(|candidate| {
        with_summary(candidate);
        candidate["copies"][LIVE]["tables"]["_copy_summary"]["rows"][0][1] = text(&format!(
            "\n{}\n",
            serde_json::to_string_pretty(&summary()).expect("serializes")
        ));
    });

    // Act / Assert
    let refused = run_gate(&inventory).await;
    assert!(
        refused
            .refusals
            .iter()
            .any(|refusal| refusal.row == "_copy_summary"
                && refusal.reason.contains("would be normalized")),
        "{refused:?}"
    );
    let accepted = run_gate(&formatted).await;
    assert_eq!(accepted.outcome, Outcome::Pass, "{accepted:?}");
}

#[tokio::test]
async fn a_missing_summary_table_requires_the_hook_to_actually_create_it() {
    // Arrange: a receipt for an already-applied hook does not prove its table remains.
    let missing = edited(|candidate| {
        with_summary(candidate);
        let copy = &mut candidate["copies"][LIVE];
        copy["schema"]
            .as_array_mut()
            .expect("schema")
            .retain(|entry| entry["name"] != "_copy_summary");
        copy["tables"]
            .as_object_mut()
            .expect("tables")
            .remove("_copy_summary");
    });
    let fresh = edited(|_| {});

    // Act / Assert: the SQL read must run after initialize_hook in both cases.
    let refused = run_gate(&missing).await;
    assert!(
        refused
            .refusals
            .iter()
            .any(|refusal| refusal.row == "_copy_summary"
                && refusal.reason.contains("_copy_summary")),
        "{refused:?}"
    );
    let accepted = run_gate(&fresh).await;
    assert_eq!(accepted.outcome, Outcome::Pass, "{accepted:?}");
}

#[tokio::test]
async fn malformed_or_mismatched_copy_summaries_are_refused_by_the_reader() {
    let cases: [(&str, fn(&mut Value)); 6] = [
        ("extra field", |summary| summary["unexpected"] = json!(true)),
        ("different database", |summary| {
            summary["database"] = json!(RETIRED)
        }),
        ("invalid database", |summary| {
            summary["database"] = json!("invalid")
        }),
        ("invalid annotation", |summary| {
            summary["summary"]["kind"] = json!("unknown")
        }),
        ("invalid position", |summary| {
            summary["position"]["epoch"] = json!(0)
        }),
        ("null annotation", |summary| {
            summary["summary"] = Value::Null
        }),
    ];
    for (name, damage) in cases {
        // Arrange
        let inventory = edited(|candidate| {
            with_summary(candidate);
            let mut record = summary();
            damage(&mut record);
            candidate["copies"][LIVE]["tables"]["_copy_summary"]["rows"][0][1] =
                text(&record.to_string());
        });

        // Act / Assert: coverage alone is not a decoder refusal.
        let report = run_gate(&inventory).await;
        assert!(
            report
                .refusals
                .iter()
                .any(|refusal| refusal.row == "_copy_summary"
                    && refusal.reason.contains("summary reader refuses")),
            "{name}: {report:?}"
        );
    }
}

#[tokio::test]
async fn copy_summary_storage_shape_identity_and_cardinality_are_checked() {
    let cases: [(&str, fn(&mut Value)); 7] = [
        ("row database mismatch", |table| {
            table["rows"][0][0] = text(RETIRED)
        }),
        ("null record", |table| {
            table["rows"][0][1] = json!({"kind":"null"})
        }),
        ("blob record", |table| {
            table["rows"][0][1] = json!({"kind":"blob","value":"7b7d"})
        }),
        ("missing record column", |table| {
            table["columns"] = json!(["database_id"]);
            table["rows"] = json!([]);
        }),
        ("missing database column", |table| {
            table["columns"] = json!(["record"]);
            table["rows"] = json!([]);
        }),
        ("multiple summaries", |table| {
            let mut row = table["rows"][0].clone();
            row[0] = text(RETIRED);
            table["rows"].as_array_mut().expect("rows").push(row);
        }),
        ("malformed JSON", |table| table["rows"][0][1] = text("{")),
    ];
    for (name, damage) in cases {
        // Arrange
        let inventory = edited(|candidate| {
            with_summary(candidate);
            damage(&mut candidate["copies"][LIVE]["tables"]["_copy_summary"]);
        });

        // Act / Assert: SQL constraints may refuse a null before the decoder.
        let report = run_gate(&inventory).await;
        assert_eq!(report.outcome, Outcome::Refused, "{name}: {report:?}");
        assert!(
            report
                .refusals
                .iter()
                .any(|refusal| refusal.row == "_copy_summary" || refusal.row == "rebuild"),
            "{name}: {report:?}"
        );
    }
}

fn with_empty_board_tables(candidate: &mut Value, project: &str) {
    let schema = BoardProjectSchema::new().expect("Board schema");
    let copy = &mut candidate["copies"][project];
    for migration in schema.migrations() {
        for statement in migration.sql.as_ref().split(';') {
            let Some(definition) = statement.trim().strip_prefix("CREATE TABLE ") else {
                continue;
            };
            let name = definition.split_whitespace().next().expect("table name");
            assert!(
                schema
                    .replicated_tables()
                    .iter()
                    .any(|table| table.as_str() == name)
            );
            copy["schema"].as_array_mut().expect("schema").push(json!({
                "type": "table", "name": name, "sql": format!("CREATE TABLE {definition}")
            }));
            // No row is inserted; the real Board DDL still rebuilds all definitions.
            copy["tables"][name] = json!({"columns": [], "rows": []});
        }
    }
}

#[tokio::test]
async fn a_retired_copy_keeps_known_empty_board_definitions() {
    // Arrange / Act: retirement clears membership and rows, retaining SQL definitions.
    let report = run_gate(&edited(|candidate| {
        with_empty_board_tables(candidate, RETIRED)
    }))
    .await;

    // Assert
    assert_eq!(report.outcome, Outcome::Pass, "{report:?}");
}

#[tokio::test]
async fn retired_board_tables_do_not_allow_nonempty_unknown_or_live_tables() {
    // Arrange
    let nonempty = edited(|candidate| {
        with_empty_board_tables(candidate, RETIRED);
        candidate["copies"][RETIRED]["tables"]["actors"] = json!({
            "columns": ["actor_id", "canonical_identity"],
            "rows": [[text(LIVE), text("{\"kind\":\"human\",\"humanId\":\"fixture\"}")]]
        });
    });
    let unknown = edited(|candidate| {
        let copy = &mut candidate["copies"][RETIRED];
        copy["schema"].as_array_mut().expect("schema").push(json!({
            "type":"table", "name":"future_main_table", "sql":"CREATE TABLE future_main_table(value TEXT)"
        }));
        copy["tables"]["future_main_table"] = json!({"columns":["value"],"rows":[]});
    });
    let live = edited(|candidate| with_empty_board_tables(candidate, LIVE));

    // Act / Assert
    for (inventory, table) in [
        (nonempty, "actors"),
        (unknown, "future_main_table"),
        (live, "actors"),
    ] {
        let report = run_gate(&inventory).await;
        assert_eq!(report.outcome, Outcome::Refused, "{report:?}");
        assert!(
            report.refusals.iter().any(|refusal| refusal.row == table),
            "{report:?}"
        );
    }
}
