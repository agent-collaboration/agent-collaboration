//! Approved empty-start schemas, executed by each host's stock SQLx migrator.

pub fn central_migrator() -> sqlx::migrate::Migrator {
    sqlx::migrate!("migrations/central")
}
pub fn workspace_migrator() -> sqlx::migrate::Migrator {
    sqlx::migrate!("migrations/workspace")
}

/// Extends a business schema with a deliberately failing upgrade for real-store tests.
#[cfg(feature = "test-support")]
pub fn failing_upgrade(original: sqlx::migrate::Migrator) -> sqlx::migrate::Migrator {
    use sqlx_core::{
        migrate::{Migration, MigrationType, Migrator},
        sql_str::SqlStr,
    };
    let mut migrations = original.iter().cloned().collect::<Vec<_>>();
    migrations.push(Migration::new(2,"fixture failing upgrade".into(),MigrationType::Simple,SqlStr::from_static("CREATE TABLE rollback_probe (probe_id TEXT PRIMARY KEY,created_at TEXT NOT NULL,updated_at TEXT NOT NULL); INSERT INTO missing_migration_fixture_table VALUES (1);"),false));
    Migrator::with_migrations(migrations)
}
