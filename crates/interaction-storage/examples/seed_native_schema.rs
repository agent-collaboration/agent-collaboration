//! Builds a fresh native Turso schema database from migration directories
//!
//! `scripts/tooling/prepare-sqlx-turso.py` runs this to get the database that checked queries
//! are described against, so offline metadata reflects the Turso engine rather than SQLite:
//!
//! ```text
//! cargo run --locked -p interaction-storage --example seed_native_schema -- <database> <migrations>...
//! ```
//!
//! Migrations run through this driver with `Migrator::run_direct` inside one owned
//! `BEGIN IMMEDIATE` transaction, followed by `PRAGMA foreign_key_check`, the same path a Router
//! store uses. The tool refuses to touch an existing file. It uses no checked macro, so it builds
//! before any offline metadata exists.

use std::{error::Error, path::PathBuf};

use sqlx::migrate::Migrator;
use sqlx_turso::{
    TursoConnectOptions,
    sqlx::{ConnectOptions, Connection},
};

type SeedResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

struct SeedRequest {
    database_path: PathBuf,
    migration_directories: Vec<PathBuf>,
}

fn parse_seed_request(arguments: Vec<String>) -> SeedResult<SeedRequest> {
    let mut arguments = arguments.into_iter().map(PathBuf::from);
    let database_path = arguments
        .next()
        .ok_or("usage: seed_native_schema <database> <migrations>...")?;
    let migration_directories: Vec<PathBuf> = arguments.collect();
    if migration_directories.is_empty() {
        return Err("at least one migrations directory is required".into());
    }
    Ok(SeedRequest {
        database_path,
        migration_directories,
    })
}

#[tokio::main]
async fn main() -> SeedResult<()> {
    let request = parse_seed_request(std::env::args().skip(1).collect())?;
    if tokio::fs::try_exists(&request.database_path).await? {
        return Err(format!(
            "{} already exists; the native schema database must be fresh",
            request.database_path.display()
        )
        .into());
    }

    let mut connection = TursoConnectOptions::new()
        .filename(&request.database_path)
        .create_if_missing(true)
        .connect()
        .await?;

    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    let mut migration_count = 0;
    for (index, directory) in request.migration_directories.iter().enumerate() {
        let mut migrator = Migrator::new(directory.as_path()).await?;
        // Independent stores and the holder-local schema have independent versions.
        // Keep the first store's ordinary history for queries that describe it.
        if index != 0 {
            migrator.dangerous_set_table_name(format!("_schema_seed_history_{index}"));
        }
        migration_count += migrator.iter().count();
        migrator.run_direct(None, &mut *transaction, false).await?;
    }
    let dangling_references = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut *transaction)
        .await?;
    if !dangling_references.is_empty() {
        return Err("migrations left dangling foreign keys".into());
    }
    transaction.commit().await?;
    connection.close().await?;

    println!(
        "seeded {} with {} migrations",
        request.database_path.display(),
        migration_count
    );
    Ok(())
}
