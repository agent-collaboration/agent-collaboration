use sqlx_core::error::{DatabaseError as _, ErrorKind};

use super::DatabaseError;

fn kind(message: &str) -> ErrorKind {
    DatabaseError::new(message.to_owned()).kind()
}

// As local D1 (workerd) reported it, 2026-09-23.
const NOT_NULL: &str = "NOT NULL constraint failed: users.email: SQLITE_CONSTRAINT \
                        (extended: SQLITE_CONSTRAINT_NOTNULL)";

#[test]
fn reads_the_extended_code() {
    let error = DatabaseError::new(NOT_NULL.to_owned());

    assert_eq!(error.code().as_deref(), Some("SQLITE_CONSTRAINT_NOTNULL"));
    assert!(matches!(error.kind(), ErrorKind::NotNullViolation));
}

#[test]
fn falls_back_to_the_primary_code() {
    let error = DatabaseError::new(
        "D1_ERROR: near \"SELEC\": syntax error at offset 0: SQLITE_ERROR".to_owned(),
    );

    assert_eq!(error.code().as_deref(), Some("SQLITE_ERROR"));
    assert!(matches!(error.kind(), ErrorKind::Other));
}

#[test]
fn maps_each_extended_constraint_code() {
    for (code, expected) in [
        ("SQLITE_CONSTRAINT_UNIQUE", "unique"),
        ("SQLITE_CONSTRAINT_PRIMARYKEY", "unique"),
        ("SQLITE_CONSTRAINT_FOREIGNKEY", "foreign key"),
        ("SQLITE_CONSTRAINT_NOTNULL", "not null"),
        ("SQLITE_CONSTRAINT_CHECK", "check"),
    ] {
        let kind = kind(&format!("x: SQLITE_CONSTRAINT (extended: {code})"));
        let actual = match kind {
            ErrorKind::UniqueViolation => "unique",
            ErrorKind::ForeignKeyViolation => "foreign key",
            ErrorKind::NotNullViolation => "not null",
            ErrorKind::CheckViolation => "check",
            _ => "other",
        };
        assert_eq!(actual, expected, "{code}");
    }
}

// Without the extended code, the constraint text still decides.
#[test]
fn maps_constraint_messages_to_kinds() {
    assert!(matches!(
        kind("D1_ERROR: UNIQUE constraint failed: users.email: SQLITE_CONSTRAINT"),
        ErrorKind::UniqueViolation
    ));
    assert!(matches!(
        kind("D1_ERROR: FOREIGN KEY constraint failed: SQLITE_CONSTRAINT"),
        ErrorKind::ForeignKeyViolation
    ));
    assert!(matches!(
        kind("D1_ERROR: NOT NULL constraint failed: users.name: SQLITE_CONSTRAINT"),
        ErrorKind::NotNullViolation
    ));
    assert!(matches!(
        kind("D1_ERROR: CHECK constraint failed: age >= 0: SQLITE_CONSTRAINT"),
        ErrorKind::CheckViolation
    ));
}

#[test]
fn anything_else_is_other() {
    assert!(matches!(
        kind("D1_ERROR: no such table: users: SQLITE_ERROR"),
        ErrorKind::Other
    ));
}
