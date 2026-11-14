use super::{alarm_time, constraint_kind, sql_value};
use central_storage::{
    ConstraintKind, CorruptReason, ReadOutcome, SafeInteger, SqlValue, StoreFailure,
};
use sqlx_durable_object::{DoArgumentValue, DoDatabaseError};

fn integer(value: i64) -> SqlValue {
    SqlValue::Integer(SafeInteger::new(value).unwrap())
}

fn database_error(message: &str) -> sqlx_core::Error {
    sqlx_core::Error::Database(Box::new(DoDatabaseError::new(message.to_owned())))
}

#[test]
fn sqlite_constraint_codes_map_to_constraint_kinds() {
    for (code, expected) in [
        ("SQLITE_CONSTRAINT_UNIQUE", Some(ConstraintKind::Unique)),
        ("SQLITE_CONSTRAINT_PRIMARYKEY", Some(ConstraintKind::Unique)),
        (
            "SQLITE_CONSTRAINT_FOREIGNKEY",
            Some(ConstraintKind::ForeignKey),
        ),
        ("SQLITE_CONSTRAINT_NOTNULL", Some(ConstraintKind::NotNull)),
        (
            "SQLITE_CONSTRAINT_CHECK",
            Some(ConstraintKind::CheckBoolean),
        ),
    ] {
        let error = database_error(&format!("x: SQLITE_CONSTRAINT (extended: {code})"));
        assert_eq!(constraint_kind(&error), expected, "{code}");
    }
    assert_eq!(
        constraint_kind(&database_error("no such table: t: SQLITE_ERROR")),
        None
    );
    assert_eq!(constraint_kind(&sqlx_core::Error::RowNotFound), None);
}

fn answer(rows: Vec<Vec<SqlValue>>) -> ReadOutcome {
    ReadOutcome {
        columns: vec!["due".into()],
        rows,
    }
}

#[test]
fn the_alarm_query_answers_one_integer_or_null() {
    assert_eq!(alarm_time(&answer(vec![vec![integer(42)]])), Ok(Some(42)));
    assert_eq!(alarm_time(&answer(vec![vec![SqlValue::Null]])), Ok(None));
    let bad = Err(StoreFailure::Corrupt {
        reason: CorruptReason::BadValue {
            column: "due".into(),
        },
    });
    assert_eq!(
        alarm_time(&answer(vec![vec![SqlValue::Text("soon".into())]])),
        bad
    );
    assert_eq!(alarm_time(&answer(Vec::new())), bad);
    assert_eq!(
        alarm_time(&answer(vec![vec![integer(1)], vec![integer(2)]])),
        bad
    );
}

#[test]
fn a_read_value_keeps_the_integer_contract() {
    assert_eq!(
        sql_value(&DoArgumentValue::Integer(SafeInteger::MAX.get()), "n"),
        Ok(SqlValue::Integer(SafeInteger::MAX))
    );
    assert_eq!(
        sql_value(&DoArgumentValue::Integer(SafeInteger::MAX.get() + 1), "n"),
        Err(StoreFailure::Corrupt {
            reason: CorruptReason::BadValue { column: "n".into() }
        })
    );
    // REAL is outside the storage contract.
    assert_eq!(
        sql_value(&DoArgumentValue::Real(1.5), "n"),
        Err(StoreFailure::Corrupt {
            reason: CorruptReason::UnknownKind { column: "n".into() }
        })
    );
}
