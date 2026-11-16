//! Approved empty-start schemas, executed by each host's stock SQLx migrator.

pub fn central_migrator() -> sqlx::migrate::Migrator {
    let production = sqlx::migrate!("migrations/central");
    #[cfg(not(feature = "test-support"))]
    return production;
    #[cfg(feature = "test-support")]
    {
        use sqlx_core::{
            migrate::{Migration, MigrationType, Migrator},
            sql_str::SqlStr,
        };
        let mut migrations = production.iter().cloned().collect::<Vec<_>>();
        let version = migrations
            .iter()
            .map(|migration| migration.version)
            .max()
            .unwrap_or(0)
            + 1;
        // Fresh test stores use the same registry as the test-support runtime.
        // Production history is unchanged; the two probe tables are fixtures.
        migrations.push(Migration::new(
            version,
            "engine probe fixture".into(),
            MigrationType::Simple,
            SqlStr::from_static(include_str!(
                "../tests/fixtures/engine_migrations/0001_engine_probe.sql"
            )),
            false,
        ));
        Migrator::with_migrations(migrations)
    }
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
    let next_version = migrations
        .iter()
        .map(|migration| migration.version)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    migrations.push(Migration::new(next_version,"fixture failing upgrade".into(),MigrationType::Simple,SqlStr::from_static("CREATE TABLE rollback_probe (probe_id TEXT PRIMARY KEY,created_at TEXT NOT NULL,updated_at TEXT NOT NULL); INSERT INTO missing_migration_fixture_table VALUES (1);"),false));
    Migrator::with_migrations(migrations)
}
