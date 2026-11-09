//! The gate over the committed candidate fixture (tools/central-migration
//! test/candidate_fixture.test.ts keeps it equal to the real inventory answer).
//! The fixture passes; each value class the TypeScript preflight refuses,
//! written into the candidate with its digest recomputed, is refused by main's
//! own decoders; a candidate that doesn't hash to its named digest is refused.

use crate::gate::{Outcome, run_gate};
use serde_json::Value;
use sha2::{Digest, Sha256};

const FIXTURE: &str = include_str!("candidate_fixture.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("the committed fixture parses")
}

/// The fixture with its candidate edited and its digest recomputed, so only main's decoders judge it.
pub(super) fn edited(edit: impl FnOnce(&mut Value)) -> String {
    let mut inventory = fixture();
    let text = inventory["candidateText"].as_str().expect("candidate text");
    let mut candidate: Value = serde_json::from_str(text).expect("candidate parses");
    edit(&mut candidate);
    let text = serde_json::to_string(&candidate).expect("candidate serializes");
    inventory["candidateDigest"] = Value::String(format!("{:x}", Sha256::digest(text.as_bytes())));
    inventory["candidateText"] = Value::String(text);
    serde_json::to_string(&inventory).expect("inventory serializes")
}

/// Every text cell of one column, for editing.
fn text_cells<'a>(object: &'a mut Value, table: &str, column: &str) -> Vec<&'a mut Value> {
    let image = &mut object["tables"][table];
    let index = image["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .position(|name| name == column)
        .unwrap_or_else(|| panic!("{table} has no column {column}"));
    image["rows"]
        .as_array_mut()
        .expect("rows")
        .iter_mut()
        .map(|row| &mut row[index])
        .filter(|cell| cell["kind"] == "text")
        .map(|cell| &mut cell["value"])
        .collect()
}

fn set_first(object: &mut Value, table: &str, column: &str, value: &str) {
    let mut cells = text_cells(object, table, column);
    let cell = cells
        .first_mut()
        .unwrap_or_else(|| panic!("{table}.{column} has no text value"));
    **cell = Value::String(value.into());
}

fn replace_in(object: &mut Value, table: &str, column: &str, from: &str, to: &str) {
    let mut replaced = 0;
    for cell in text_cells(object, table, column) {
        let text = cell.as_str().expect("text").to_owned();
        if text.contains(from) {
            *cell = Value::String(text.replace(from, to));
            replaced += 1;
        }
    }
    assert!(replaced > 0, "{table}.{column} never contains {from}");
}

#[tokio::test]
async fn the_committed_candidate_passes_and_the_pass_names_its_digest_and_run_time() {
    // Arrange
    let inventory = fixture();

    // Act
    let report = run_gate(FIXTURE).await;

    // Assert
    assert_eq!(report.refusals, Vec::new());
    assert_eq!(report.outcome, Outcome::Pass);
    assert_eq!(
        report.candidate_digest.as_deref(),
        inventory["candidateDigest"].as_str()
    );
    assert_eq!(report.run_time_millis, inventory["runTimeMillis"].as_i64());
}

type Edit = fn(&mut Value);

const REFUSED: [(&str, Edit, &str, &str); 9] = [
    (
        "a 300-byte machine label",
        |candidate| {
            set_first(
                &mut candidate["fleet"],
                "machine_enrollments",
                "machine_label",
                &"x".repeat(300),
            );
        },
        "fleet",
        "machine_enrollments ",
    ),
    (
        "a machine label with NUL",
        |candidate| {
            set_first(
                &mut candidate["fleet"],
                "machine_enrollments",
                "machine_label",
                "Sun\u{0}book",
            );
        },
        "fleet",
        "machine_enrollments ",
    ),
    (
        "a lease-table integer written 1.0",
        |candidate| {
            replace_in(
                &mut candidate["fleet"],
                "_fleet_sync_databases",
                "record",
                "\"change\":1",
                "\"change\":1.0",
            );
        },
        "fleet",
        "lease tables",
    ),
    (
        "a lease record with an extra constructor field",
        |candidate| {
            replace_in(
                &mut candidate["fleet"],
                "_fleet_sync_databases",
                "record",
                "{\"database\"",
                "{\"constructor\":0,\"database\"",
            );
        },
        "fleet",
        "lease tables",
    ),
    (
        "a lease-table time written -0",
        |candidate| {
            replace_in(
                &mut candidate["fleet"],
                "_fleet_sync_databases",
                "record",
                "\"changed_at\":1794225600000",
                "\"changed_at\":-0",
            );
        },
        "fleet",
        "lease tables",
    ),
    (
        "a lease-table string with a lone surrogate",
        |candidate| {
            replace_in(
                &mut candidate["fleet"],
                "_fleet_sync_transfer_cancels",
                "record",
                "\"changed my mind\"",
                "\"changed my mind\\uDC00\"",
            );
        },
        "fleet",
        "lease tables",
    ),
    (
        "a blank directory name",
        |candidate| {
            set_first(
                &mut candidate["fleet"],
                "fleet_project_directory",
                "name",
                "   ",
            );
        },
        "fleet",
        "fleet_project_directory ",
    ),
    (
        "an unknown madeIn kind",
        |candidate| {
            set_first(
                &mut candidate["fleet"],
                "fleet_workspace_adrs",
                "made_in_kind",
                "bogus",
            );
        },
        "fleet",
        "fleet_workspace_adrs ",
    ),
    (
        "a Copy schema fingerprint with a lone surrogate",
        |candidate| {
            let copies = candidate["copies"].as_object_mut().expect("copies");
            let copy = copies.values_mut().next().expect("one copy");
            replace_in(
                copy,
                "_fleet_sync_copy",
                "record",
                "\"schema-fingerprint\"",
                "\"schema-fingerprint\\uDC00\"",
            );
        },
        "copy ",
        "_fleet_sync_copy",
    ),
];

#[tokio::test]
async fn each_value_class_the_preflight_refuses_is_refused_by_mains_decoders() {
    for (name, edit, object, row) in REFUSED {
        // Arrange
        let inventory = edited(edit);

        // Act
        let report = run_gate(&inventory).await;

        // Assert: refused, at the edited object and row.
        assert_eq!(report.outcome, Outcome::Refused, "{name}: {report:?}");
        assert!(
            report
                .refusals
                .iter()
                .any(|refusal| refusal.object.starts_with(object) && refusal.row.starts_with(row)),
            "{name}: expected a refusal at {object} {row}, got {:?}",
            report.refusals
        );
    }
}

#[tokio::test]
async fn a_candidate_that_does_not_hash_to_its_named_digest_is_refused() {
    // Arrange: the candidate changed after its digest was taken.
    let mut inventory = fixture();
    let text = inventory["candidateText"]
        .as_str()
        .expect("text")
        .to_owned();
    inventory["candidateText"] = Value::String(text.replacen("Sunbook", "Sunbook2", 1));

    // Act
    let report = run_gate(&serde_json::to_string(&inventory).expect("serializes")).await;

    // Assert
    assert_eq!(report.outcome, Outcome::Refused);
    assert_eq!(report.refusals.len(), 1);
    assert_eq!(report.refusals[0].object, "candidate");
}

#[tokio::test]
async fn an_inventory_with_stops_is_refused() {
    let mut inventory = fixture();
    inventory["stops"] = serde_json::json!([{ "object": "fleet", "reason": "R7" }]);
    let report = run_gate(&serde_json::to_string(&inventory).expect("serializes")).await;
    assert_eq!(report.outcome, Outcome::Refused);
    assert_eq!(report.refusals[0].object, "inventory");
}

/// Sets one cell of the first row of a table to a typed value.
fn set_cell(object: &mut Value, table: &str, column: &str, value: Value) {
    let image = &mut object["tables"][table];
    let index = image["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .position(|name| name == column)
        .unwrap_or_else(|| panic!("{table} has no column {column}"));
    image["rows"][0][index] = value;
}

#[tokio::test]
async fn a_legacy_unknown_repository_attachment_without_its_import_is_refused_and_a_valid_one_passes()
 {
    // Arrange: the Reviewer's G1 row — legacy time without a router time or import digest.
    let legacy = edited(|candidate| {
        let fleet = &mut candidate["fleet"];
        set_cell(
            fleet,
            "fleet_repository_attachments",
            "time_basis",
            serde_json::json!({ "kind": "text", "value": "legacy_unknown" }),
        );
        set_cell(
            fleet,
            "fleet_repository_attachments",
            "router_time",
            serde_json::json!({ "kind": "null" }),
        );
        set_cell(
            fleet,
            "fleet_repository_attachments",
            "import_digest",
            serde_json::json!({ "kind": "null" }),
        );
    });

    // Act
    let refused = run_gate(&legacy).await;
    let control = run_gate(&edited(|_| {})).await;

    // Assert: refused at the attachment's workspace by main's repository decoder; the unedited row passes.
    assert_eq!(refused.outcome, Outcome::Refused, "{refused:?}");
    assert!(
        refused
            .refusals
            .iter()
            .any(|refusal| refusal.object == "fleet"
                && refusal.row.starts_with("fleet_repository_attachments ")),
        "{:?}",
        refused.refusals
    );
    assert_eq!(control.outcome, Outcome::Pass, "{control:?}");
}

#[tokio::test]
async fn a_table_without_gate_coverage_or_a_row_in_a_must_be_empty_table_is_refused() {
    // Arrange: an unknown table, and a row where the migration carries none.
    let unknown = edited(|candidate| {
        candidate["fleet"]["tables"]["fleet_unseen_family"] =
            serde_json::json!({ "columns": ["value"], "rows": [] });
        candidate["fleet"]["schema"]
            .as_array_mut()
            .expect("schema")
            .push(serde_json::json!({
                "type": "table",
                "name": "fleet_unseen_family",
                "sql": "CREATE TABLE fleet_unseen_family(value TEXT)"
            }));
    });
    let carried = edited(|candidate| {
        let table = &mut candidate["fleet"]["tables"]["fleet_feed_readers"];
        let width = table["columns"].as_array().expect("columns").len();
        table["rows"] = serde_json::json!([vec![serde_json::json!({ "kind": "null" }); width]]);
    });

    // Act
    let unknown = run_gate(&unknown).await;
    let carried = run_gate(&carried).await;

    // Assert
    assert!(
        unknown
            .refusals
            .iter()
            .any(|refusal| refusal.row == "fleet_unseen_family"
                && refusal.reason.contains("no gate coverage")),
        "{:?}",
        unknown.refusals
    );
    assert!(
        carried
            .refusals
            .iter()
            .any(|refusal| refusal.row == "fleet_feed_readers"
                && refusal.reason.contains("main decodes them")),
        "{:?}",
        carried.refusals
    );
}

#[test]
fn every_table_mains_fleet_migrations_create_is_decoded_or_must_be_empty() {
    use crate::table_coverage::{Coverage, FLEET, coverage};
    // Main's own Fleet migration sets: central-fleet, admin imports and the registry set.
    let core =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../collaboration-central-core");
    let mut missing = Vec::new();
    for directory in [
        "core_migrations",
        "admin_import_migrations",
        "registry_migrations",
    ] {
        for entry in std::fs::read_dir(core.join(directory)).expect("main's migrations") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("sql") {
                continue;
            }
            let sql = std::fs::read_to_string(&path).expect("migration SQL");
            for statement in sql.split("CREATE TABLE ").skip(1) {
                let table: String = statement
                    .trim_start_matches("IF NOT EXISTS ")
                    .chars()
                    .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
                    .collect();
                match coverage(FLEET, &table) {
                    Some(
                        Coverage::Decoded(_) | Coverage::DecodedWith(_) | Coverage::MustBeEmpty(_),
                    ) => {}
                    other => missing.push(format!("{table}: {other:?}")),
                }
            }
        }
    }
    assert_eq!(missing, Vec::<String>::new());
}

/// Adds one row to a table of the first Copy (the held-at-three project).
pub(super) fn add_copy_row(candidate: &mut Value, table: &str, row: Value) {
    let copies = candidate["copies"].as_object_mut().expect("copies");
    let copy = copies.values_mut().next().expect("one copy");
    copy["tables"][table]["rows"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("{table} rows"))
        .push(row);
}

pub(super) fn text(value: &str) -> Value {
    serde_json::json!({ "kind": "text", "value": value })
}
pub(super) fn integer(value: i64) -> Value {
    serde_json::json!({ "kind": "integer", "value": value.to_string() })
}

#[tokio::test]
async fn a_receipt_whose_digest_is_not_a_piece_digest_is_refused_and_a_valid_receipt_passes() {
    // Arrange
    let receipt = |digest: &str| {
        let digest = digest.to_owned();
        edited(move |candidate| {
            add_copy_row(
                candidate,
                "_fleet_sync_receipts",
                serde_json::json!([text("piece-identity"), text(&digest)]),
            );
        })
    };

    // Act
    let valid = run_gate(&receipt(&"a".repeat(64))).await;
    let refused = run_gate(&receipt("not-a-digest")).await;

    // Assert
    assert_eq!(valid.outcome, Outcome::Pass, "{valid:?}");
    assert!(
        refused
            .refusals
            .iter()
            .any(|refusal| refusal.row.starts_with("_fleet_sync_receipts row")),
        "{:?}",
        refused.refusals
    );
}

#[test]
fn every_table_the_receiver_corpus_creates_is_decoded_must_be_empty_or_a_named_ledger() {
    use crate::table_coverage::{COPY, Coverage, coverage};
    let ledgers = [
        "_fleet_sync_hook_migrations",
        "_fleet_sync_legacy_hook_migrations",
    ];
    let mut missing = Vec::new();
    for migration in fleet_sync_receiver::RECEIVER_MIGRATOR.iter() {
        for statement in migration.sql.as_ref().split("CREATE TABLE ").skip(1) {
            let table: String = statement
                .trim_start_matches("IF NOT EXISTS ")
                .chars()
                .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
                .collect();
            match coverage(COPY, &table) {
                Some(Coverage::Decoded(_) | Coverage::MustBeEmpty(_)) => {}
                Some(Coverage::NotReadByMain(_)) if ledgers.contains(&table.as_str()) => {}
                other => missing.push(format!("{table}: {other:?}")),
            }
        }
    }
    assert_eq!(missing, Vec::<String>::new());
}
