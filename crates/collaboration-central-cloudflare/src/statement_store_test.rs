use super::{alarm_time, constraint_kind};
use collaboration_central_core::storage::{
    ConstraintKind, CorruptReason, ReadOutcome, SqlValue, StoreFailure,
};
use sqlx_durable_object::DoDatabaseError;

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
    assert_eq!(
        alarm_time(&answer(vec![vec![SqlValue::Integer(42)]])),
        Ok(Some(42))
    );
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
        alarm_time(&answer(vec![
            vec![SqlValue::Integer(1)],
            vec![SqlValue::Integer(2)]
        ])),
        bad
    );
}
