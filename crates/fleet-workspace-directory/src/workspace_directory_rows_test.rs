//! Direct production-decoder oracles: stored rows fail closed, never normalize.
use super::decode;
use central_storage::{CorruptReason, ReadOutcome, SqlValue, StoreFailure};
use workspace_model::WorkspaceState;

const WORKSPACE: &str = "019f0000-0000-7000-8000-00000000da01";
const CREATED: &str = "2030-03-17T17:46:40Z";
const UPDATED: &str = "2030-03-17T18:46:40Z";

fn text(value: impl Into<String>) -> SqlValue {
    SqlValue::Text(value.into())
}

fn stored_row() -> ReadOutcome {
    ReadOutcome {
        columns: [
            "workspace_id",
            "name",
            "description",
            "state",
            "created_at",
            "updated_at",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        rows: vec![vec![
            text(WORKSPACE),
            text("Research"),
            text(""),
            text("active"),
            text(CREATED),
            text(UPDATED),
        ]],
    }
}

fn replace_cell(rows: &mut ReadOutcome, column: &str, value: SqlValue) {
    let index = rows.columns.iter().position(|name| name == column).unwrap();
    rows.rows.first_mut().unwrap()[index] = value;
}

fn assert_bad_value(rows: &ReadOutcome, field: &str) {
    match decode(rows) {
        Err(error) => assert_eq!(
            error,
            StoreFailure::Corrupt {
                reason: CorruptReason::BadValue {
                    column: field.to_owned()
                },
            }
        ),
        Ok(_) => panic!("production directory decoder accepted corrupt {field}"),
    }
}

#[test]
fn empty_directory_read_is_absent() {
    let mut rows = stored_row();
    rows.rows.clear();
    assert!(decode(&rows).unwrap().is_none());
}

#[test]
fn canonical_names_and_descriptions_keep_their_utf8_byte_boundary_values() {
    // Literal write contract: 256-byte name, 2048-byte description. Multibyte
    // controls reject a character-count interpretation of either byte limit.
    for (name, description) in [
        ("Research".to_owned(), String::new()),
        ("a".repeat(256), "d".repeat(2048)),
        ("é".repeat(128), "é".repeat(1024)),
        ("😀".repeat(64), "😀".repeat(512)),
    ] {
        for (state, expected_state) in [
            ("active", WorkspaceState::Active),
            ("archived", WorkspaceState::Archived),
        ] {
            let mut rows = stored_row();
            replace_cell(&mut rows, "name", text(name.clone()));
            replace_cell(&mut rows, "description", text(description.clone()));
            replace_cell(&mut rows, "state", text(state));

            let decoded = decode(&rows).unwrap().unwrap();

            assert_eq!(decoded.workspace_id.to_string(), WORKSPACE);
            assert_eq!(decoded.name.as_str(), name);
            assert_eq!(decoded.description.as_str(), description);
            assert_eq!(decoded.state, expected_state);
        }
    }
}

#[test]
fn padded_empty_and_over_256_byte_names_are_rejected_without_trimming() {
    for name in [
        " Research ".to_owned(),
        "\tResearch\n".to_owned(),
        String::new(),
        "a".repeat(257),
        format!("{}a", "é".repeat(128)),
    ] {
        let mut rows = stored_row();
        replace_cell(&mut rows, "name", text(name));

        assert_bad_value(&rows, "workspaces.name");
    }
}

#[test]
fn stored_descriptions_over_the_2048_byte_write_bound_are_corrupt() {
    // The shared Description type admits up to16KiB; those values must still
    // fail the narrower actual directory write invariant at this decoder.
    for description in [
        "d".repeat(2049),
        format!("{}a", "é".repeat(1024)),
        format!("{}a", "😀".repeat(512)),
        "d".repeat(16 * 1024 + 1),
    ] {
        let mut rows = stored_row();
        replace_cell(&mut rows, "description", text(description));

        assert_bad_value(&rows, "workspaces.description");
    }
}

#[test]
fn unknown_stored_state_is_rejected_instead_of_becoming_active() {
    for state in ["archiving", "enabled", "", "Active"] {
        let mut rows = stored_row();
        replace_cell(&mut rows, "state", text(state));

        assert_bad_value(&rows, "workspaces.state");
    }
}

#[test]
fn each_unparseable_stored_timestamp_names_its_own_field() {
    for column in ["created_at", "updated_at"] {
        for value in ["not-a-timestamp", "2030-99-17T17:46:40Z", ""] {
            let mut rows = stored_row();
            replace_cell(&mut rows, column, text(value));

            assert_bad_value(&rows, &format!("workspaces.{column}"));
        }
    }
}

#[test]
fn malformed_or_non_v7_workspace_identity_is_corrupt() {
    for id in ["not-a-uuid", "019f0000-0000-4000-8000-00000000da01", ""] {
        let mut rows = stored_row();
        replace_cell(&mut rows, "workspace_id", text(id));

        assert_bad_value(&rows, "workspaces.workspace_id");
    }
}

#[test]
fn duplicate_directory_rows_are_corrupt_even_when_identical() {
    let mut rows = stored_row();
    rows.rows.push(rows.rows.first().unwrap().clone());

    assert_bad_value(&rows, "workspaces.workspace_id");
}

#[test]
fn non_text_sql_cells_fail_at_the_existing_column_decode_boundary() {
    for column in [
        "workspace_id",
        "name",
        "description",
        "state",
        "created_at",
        "updated_at",
    ] {
        for value in [
            SqlValue::Null,
            SqlValue::Integer(7.try_into().unwrap()),
            SqlValue::Blob(vec![7]),
        ] {
            let mut rows = stored_row();
            replace_cell(&mut rows, column, value);

            // The shared string codec already names this transport column;
            // semantic constructor failures above use the table-qualified name.
            assert_bad_value(&rows, column);
        }
    }
}
