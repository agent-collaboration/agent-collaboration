//! Generate expected fingerprints from every real migration prefix on native Turso.

use message_board_storage::{PROJECT_MIGRATOR, schema_fingerprint::schema_fingerprint};
use sqlx::{Connection, migrate::Migrator};
use sqlx_turso::{TursoConnectOptions, sqlx::ConnectOptions};
use std::{collections::BTreeMap, error::Error, path::PathBuf};

type GenerateResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::main]
async fn main() -> GenerateResult {
    let output = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: generate_project_fingerprints <output.json>")?;
    let mut connection = TursoConnectOptions::new().connect().await?;
    sqlx::query("PRAGMA capture_data_changes_conn('full')")
        .execute(&mut connection)
        .await?;
    let mut expected = BTreeMap::new();
    for migration in PROJECT_MIGRATOR.iter() {
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut connection)
            .await?;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let prefix = Migrator::with_migrations(
            PROJECT_MIGRATOR
                .iter()
                .filter(|step| step.version <= migration.version)
                .cloned()
                .collect(),
        );
        prefix.run_direct(None, &mut *transaction, false).await?;
        if !sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *transaction)
            .await?
            .is_empty()
        {
            return Err("migration left a dangling foreign key".into());
        }
        let fingerprint = schema_fingerprint(&mut *transaction).await?;
        transaction.commit().await?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut connection)
            .await?;
        expected.insert(migration.version.to_string(), fingerprint);
    }
    connection.close().await?;
    std::fs::write(
        output,
        format!("{}\n", serde_json::to_string_pretty(&expected)?),
    )?;
    println!("generated {} native migration fingerprints", expected.len());
    Ok(())
}
