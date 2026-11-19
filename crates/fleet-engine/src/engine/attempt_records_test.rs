use super::{decode_attempt, failure_detail};
use central_engine_model::{AttemptState, ItemFailureClass};
use central_storage::{ConstraintKind, ReadOutcome, SqlValue};

fn text(value: &str) -> SqlValue {
    SqlValue::Text(value.into())
}
fn row() -> ReadOutcome {
    ReadOutcome {
        columns: [
            "attempt_id",
            "machine_kind",
            "subject_key",
            "state",
            "failure_count",
            "next_try_at",
            "last_failure",
            "last_failure_detail",
            "first_failed_at",
            "last_failed_at",
            "retired_at",
            "created_at",
            "updated_at",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        rows: vec![vec![
            text("01890f2e-7b4c-7cc0-98c4-000000000001"),
            text("wake_timer"),
            text("01890f2e-7b4c-7cc0-98c4-000000000002"),
            text("failing"),
            SqlValue::Integer(1.try_into().unwrap()),
            text("1970-01-01T00:05:00.000Z"),
            text("corrupt_row"),
            text("wakes.target"),
            text("1970-01-01T00:00:00.000Z"),
            text("1970-01-01T00:00:00.000Z"),
            SqlValue::Null,
            text("1970-01-01T00:00:00.000Z"),
            text("1970-01-01T00:00:00.000Z"),
        ]],
    }
}
fn set(outcome: &mut ReadOutcome, name: &str, value: SqlValue) {
    let index = outcome
        .columns
        .iter()
        .position(|column| column == name)
        .unwrap();
    outcome.rows[0][index] = value;
}

#[test]
fn valid_failing_and_retired_rows_decode_as_distinct_states() {
    let mut outcome = row();
    let attempt = decode_attempt(&outcome).unwrap().unwrap().record.unwrap();
    assert_eq!(attempt.attempt_id.get_version_num(), 7);
    assert_eq!(attempt.failure_count.get(), 1);
    assert_eq!(
        attempt.state,
        AttemptState::Failing {
            next_try_at: chrono::DateTime::from_timestamp_millis(300_000).unwrap()
        }
    );
    set(&mut outcome, "state", text("retired"));
    set(
        &mut outcome,
        "failure_count",
        SqlValue::Integer(2.try_into().unwrap()),
    );
    set(&mut outcome, "next_try_at", SqlValue::Null);
    set(&mut outcome, "retired_at", text("1970-01-01T00:05:00.000Z"));
    set(
        &mut outcome,
        "last_failed_at",
        text("1970-01-01T00:05:00.000Z"),
    );
    let attempt = decode_attempt(&outcome).unwrap().unwrap().record.unwrap();
    assert_eq!(
        attempt.state,
        AttemptState::Retired {
            retired_at: chrono::DateTime::from_timestamp_millis(300_000).unwrap()
        }
    );
}

#[test]
fn unknown_control_tags_are_rejected_with_raw_evidence_and_guard_values() {
    for field in ["state", "last_failure"] {
        let mut outcome = row();
        set(&mut outcome, field, text("damaged-control-tag"));
        let stored = decode_attempt(&outcome).unwrap().unwrap();
        let error = stored.record.unwrap_err();
        assert_eq!(error.column, format!("central_work_attempts.{field}"));
        assert_eq!(error.raw_value, "damaged-control-tag");
        assert_eq!(
            error.failure_class(),
            ItemFailureClass::CorruptRow {
                column: format!("central_work_attempts.{field}"),
                raw_value: Some("damaged-control-tag".into())
            }
        );
        let raw = if field == "state" {
            stored.guard.state
        } else {
            stored.guard.last_failure
        };
        assert_eq!(raw, text("damaged-control-tag"));
    }
}

#[test]
fn invalid_keys_counts_times_and_variant_fields_never_default() {
    for (field, value) in [
        ("attempt_id", text("01890f2e-7b4c-4cc0-98c4-000000000001")),
        ("failure_count", SqlValue::Integer(0.try_into().unwrap())),
        ("failure_count", text("1")),
        ("next_try_at", text("not-a-time")),
        ("next_try_at", text("2262-04-11T23:47:17.854Z")),
        ("retired_at", text("1970-01-01T00:05:00.000Z")),
        ("created_at", text("not-a-time")),
    ] {
        let mut outcome = row();
        set(&mut outcome, field, value);
        let error = decode_attempt(&outcome)
            .unwrap()
            .unwrap()
            .record
            .unwrap_err();
        assert_eq!(error.column, format!("central_work_attempts.{field}"));
    }
}

#[test]
fn every_closed_failure_class_decodes_its_declared_payload() {
    for (tag, detail, expected) in [
        (
            "corrupt_row",
            Some("wakes.target\nnot-json"),
            ItemFailureClass::CorruptRow {
                column: "wakes.target".into(),
                raw_value: Some("not-json".into()),
            },
        ),
        (
            "constraint_violated",
            Some("unique"),
            ItemFailureClass::ConstraintViolated {
                constraint: ConstraintKind::Unique,
            },
        ),
        (
            "constraint_violated",
            Some("foreign_key"),
            ItemFailureClass::ConstraintViolated {
                constraint: ConstraintKind::ForeignKey,
            },
        ),
        (
            "constraint_violated",
            Some("not_null"),
            ItemFailureClass::ConstraintViolated {
                constraint: ConstraintKind::NotNull,
            },
        ),
        (
            "constraint_violated",
            Some("check_boolean"),
            ItemFailureClass::ConstraintViolated {
                constraint: ConstraintKind::CheckBoolean,
            },
        ),
        (
            "statement_rejected",
            None,
            ItemFailureClass::StatementRejected,
        ),
        (
            "refused",
            Some("already_finished"),
            ItemFailureClass::Refused {
                refusal: "already_finished".into(),
            },
        ),
        (
            "other_store_unavailable",
            None,
            ItemFailureClass::OtherStoreUnavailable,
        ),
    ] {
        let mut outcome = row();
        set(&mut outcome, "last_failure", text(tag));
        set(
            &mut outcome,
            "last_failure_detail",
            detail.map_or(SqlValue::Null, text),
        );
        let actual = decode_attempt(&outcome)
            .unwrap()
            .unwrap()
            .record
            .unwrap()
            .last_failure;
        assert_eq!(actual, expected);
        assert_eq!(failure_detail(&actual).as_deref(), detail);
    }
}
