//! Test-only schema and statements both hosts run to prove the storage boundary
//! (Verification O1). Built only with the non-default `test-support` feature.

use crate::storage::{
    AlarmUpdate, GuardedStatement, RowExpectation, SafeInteger, SqlValue, Statement,
};
use sqlx_core::migrate::{Migration, MigrationType, Migrator};
use sqlx_core::sql_str::SqlStr;
use std::borrow::Cow;

const WIDGETS_MIGRATION: &str =
    include_str!("../tests/fixtures/storage_migrations/0001_widgets.sql");
const FAILING_MIGRATION: &str =
    include_str!("../tests/fixtures/storage_migrations/0002_fails_on_second_statement.sql");

fn widgets_migration() -> Migration {
    Migration::new(
        1,
        Cow::Borrowed("widgets"),
        MigrationType::Simple,
        SqlStr::from_static(WIDGETS_MIGRATION),
        false,
    )
}

/// The widgets schema: an owner table, a widgets table with an FK, a NOT NULL name,
/// a boolean CHECK and a partial unique index on live names.
#[must_use]
pub fn widgets_migrator() -> Migrator {
    Migrator::with_migrations(vec![widgets_migration()])
}

/// The widgets schema followed by a migration whose second statement fails.
#[must_use]
pub fn failing_second_statement_migrator() -> Migrator {
    Migrator::with_migrations(vec![
        widgets_migration(),
        Migration::new(
            2,
            Cow::Borrowed("fails on second statement"),
            MigrationType::Simple,
            SqlStr::from_static(FAILING_MIGRATION),
            false,
        ),
    ])
}

/// The table the failing migration's first statement creates; it must never exist.
pub const FAILING_MIGRATION_FIRST_TABLE: &str = "widget_labels";

const INSERT_OWNER: &str =
    "INSERT INTO widget_owners (owner_id, created_at, updated_at) VALUES (?, ?, ?)";
const INSERT_WIDGET: &str = "INSERT INTO widgets (widget_id, owner_id, name, active, retired_at, due_at_ms, created_at, updated_at) VALUES (?, ?, ?, ?, NULL, ?, ?, ?)";
const RETIRE_LIVE_WIDGET: &str =
    "UPDATE widgets SET retired_at = ?, updated_at = ? WHERE widget_id = ? AND retired_at IS NULL";
const LIST_WIDGETS: &str = "SELECT widget_id, owner_id, name, active, retired_at, due_at_ms FROM widgets ORDER BY widget_id";
const COUNT_OWNERS: &str = "SELECT COUNT(*) AS owners FROM widget_owners";
const EARLIEST_WIDGET_DUE: &str =
    "SELECT MIN(due_at_ms) AS earliest_due_ms FROM widgets WHERE retired_at IS NULL";
const LIST_TABLES: &str = "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name";
const LIST_APPLIED_MIGRATIONS: &str = "SELECT version, description, installed_on, success, checksum, execution_time FROM _sqlx_migrations ORDER BY version";

const STAMP: &str = "2026-11-12T00:00:00Z";

fn text(value: &str) -> SqlValue {
    SqlValue::Text(value.to_owned())
}

#[must_use]
pub fn insert_owner(owner_id: &str, expect: RowExpectation) -> GuardedStatement {
    GuardedStatement {
        statement: Statement {
            sql: INSERT_OWNER,
            args: vec![text(owner_id), text(STAMP), text(STAMP)],
        },
        expect,
    }
}

/// A widget insert; `name` and `active` are raw values so a case can bind NULL or a
/// non-boolean to reach NOT NULL and the boolean CHECK.
#[must_use]
pub fn insert_widget(
    widget_id: &str,
    owner_id: &str,
    name: SqlValue,
    active: SqlValue,
    due_at_ms: Option<SafeInteger>,
) -> GuardedStatement {
    GuardedStatement {
        statement: Statement {
            sql: INSERT_WIDGET,
            args: vec![
                text(widget_id),
                text(owner_id),
                name,
                active,
                due_at_ms.map_or(SqlValue::Null, SqlValue::Integer),
                text(STAMP),
                text(STAMP),
            ],
        },
        expect: RowExpectation::Exactly(1),
    }
}

/// Retires a live widget; guarded so a missing or already retired widget fails the batch.
#[must_use]
pub fn retire_live_widget(widget_id: &str) -> GuardedStatement {
    GuardedStatement {
        statement: Statement {
            sql: RETIRE_LIVE_WIDGET,
            args: vec![text(STAMP), text(STAMP), text(widget_id)],
        },
        expect: RowExpectation::Exactly(1),
    }
}

#[must_use]
pub fn list_widgets() -> Statement {
    Statement {
        sql: LIST_WIDGETS,
        args: Vec::new(),
    }
}

#[must_use]
pub fn count_owners() -> Statement {
    Statement {
        sql: COUNT_OWNERS,
        args: Vec::new(),
    }
}

#[must_use]
pub fn list_tables() -> Statement {
    Statement {
        sql: LIST_TABLES,
        args: Vec::new(),
    }
}

#[must_use]
pub fn list_applied_migrations() -> Statement {
    Statement {
        sql: LIST_APPLIED_MIGRATIONS,
        args: Vec::new(),
    }
}

/// Sets the object's alarm to the earliest live widget's due time, or clears it.
#[must_use]
pub fn alarm_from_earliest_widget_due() -> AlarmUpdate {
    AlarmUpdate::SetFromQuery(Statement {
        sql: EARLIEST_WIDGET_DUE,
        args: Vec::new(),
    })
}
