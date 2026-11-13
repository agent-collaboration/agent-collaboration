use super::{HistoryFault, is_plain_identifier, validate_source};
use sqlx_core::migrate::{Migration, MigrationType, Migrator};
use sqlx_core::sql_str::SqlStr;
use std::borrow::Cow;

fn migration(version: i64, no_tx: bool) -> Migration {
    Migration::new(
        version,
        Cow::Borrowed("case"),
        MigrationType::Simple,
        SqlStr::from_static("SELECT 1"),
        no_tx,
    )
}

fn unsorted(migrations: Vec<Migration>) -> Migrator {
    Migrator {
        migrations: Cow::Owned(migrations),
        ..Migrator::DEFAULT
    }
}

#[test]
fn an_empty_source_is_allowed_and_has_no_target() {
    assert!(matches!(validate_source(&unsorted(Vec::new())), Ok(None)));
}

#[test]
fn an_ordered_source_targets_its_highest_version() {
    let source = unsorted(vec![migration(1, false), migration(3, false)]);
    assert!(matches!(validate_source(&source), Ok(Some(3))));
}

#[test]
fn a_repeated_or_descending_version_is_refused() {
    let repeated = unsorted(vec![migration(1, false), migration(1, false)]);
    assert!(matches!(
        validate_source(&repeated),
        Err(HistoryFault::Unordered { version: 1 })
    ));
    let descending = unsorted(vec![migration(2, false), migration(1, false)]);
    assert!(matches!(
        validate_source(&descending),
        Err(HistoryFault::Unordered { version: 1 })
    ));
}

#[test]
fn a_migration_outside_the_transaction_is_refused() {
    let source = unsorted(vec![migration(1, true)]);
    assert!(matches!(
        validate_source(&source),
        Err(HistoryFault::NoTransaction { version: 1 })
    ));
}

#[test]
fn only_plain_identifiers_name_the_history() {
    assert!(is_plain_identifier("_sqlx_migrations"));
    assert!(!is_plain_identifier("1table"));
    assert!(!is_plain_identifier("main.history"));
    assert!(!is_plain_identifier(""));
}
