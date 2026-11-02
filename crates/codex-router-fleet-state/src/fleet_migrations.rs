//! Only this migrator may establish authority over this Router's fleet schema.
use sqlx::{Row, SqliteConnection, SqlitePool};

pub(crate) static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FleetStateOpenError {
    #[error("fleet store unavailable")]
    Unavailable,
    #[error("fleet store schema or migration history is incompatible")]
    InvalidSchema,
}

pub(crate) async fn migrate(pool: &SqlitePool) -> Result<(), FleetStateOpenError> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|_| FleetStateOpenError::Unavailable)?;
    let history: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='_sqlx_migrations'",
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| FleetStateOpenError::InvalidSchema)?;
    if history == 0 {
        let objects: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'")
                .fetch_one(&mut *transaction)
                .await
                .map_err(|_| FleetStateOpenError::InvalidSchema)?;
        if objects != 0 {
            return Err(FleetStateOpenError::InvalidSchema);
        }
    }
    MIGRATOR
        .run_direct(None, &mut *transaction, false)
        .await
        .map_err(|_| FleetStateOpenError::InvalidSchema)?;
    validate_current(&mut transaction).await?;
    transaction
        .commit()
        .await
        .map_err(|_| FleetStateOpenError::Unavailable)
}

pub(crate) async fn validate_current(
    connection: &mut SqliteConnection,
) -> Result<(), FleetStateOpenError> {
    let applied =
        sqlx::query("SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut *connection)
            .await
            .map_err(|_| FleetStateOpenError::InvalidSchema)?;
    let migrations: Vec<_> = MIGRATOR.iter().collect();
    if applied.len() != migrations.len() {
        return Err(FleetStateOpenError::InvalidSchema);
    }
    for (row, migration) in applied.iter().zip(migrations) {
        let version: i64 = row
            .try_get("version")
            .map_err(|_| FleetStateOpenError::InvalidSchema)?;
        let success: bool = row
            .try_get("success")
            .map_err(|_| FleetStateOpenError::InvalidSchema)?;
        let checksum: Vec<u8> = row
            .try_get("checksum")
            .map_err(|_| FleetStateOpenError::InvalidSchema)?;
        if !success
            || version != migration.version
            || checksum.as_slice() != migration.checksum.as_ref()
        {
            return Err(FleetStateOpenError::InvalidSchema);
        }
    }
    let columns = sqlx::query("PRAGMA table_info(fleet_login)")
        .fetch_all(&mut *connection)
        .await
        .map_err(|_| FleetStateOpenError::InvalidSchema)?;
    let names = [
        "enrollment_id",
        "sign_in",
        "organization_id",
        "machine_environment",
        "receiver_id",
        "relay_peer_id",
    ];
    if columns.len() != names.len() {
        return Err(FleetStateOpenError::InvalidSchema);
    }
    for (column, expected) in columns.iter().zip(names) {
        let name: String = column
            .try_get("name")
            .map_err(|_| FleetStateOpenError::InvalidSchema)?;
        let kind: String = column
            .try_get("type")
            .map_err(|_| FleetStateOpenError::InvalidSchema)?;
        let required: bool = column
            .try_get("notnull")
            .map_err(|_| FleetStateOpenError::InvalidSchema)?;
        let primary: i64 = column
            .try_get("pk")
            .map_err(|_| FleetStateOpenError::InvalidSchema)?;
        if name != expected
            || kind != "TEXT"
            || !required
            || primary != i64::from(expected == "enrollment_id")
        {
            return Err(FleetStateOpenError::InvalidSchema);
        }
    }
    let index: Option<String> = sqlx::query_scalar(
        "SELECT sql FROM sqlite_schema WHERE type='index' AND name='fleet_login_single_row'",
    )
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| FleetStateOpenError::InvalidSchema)?;
    if index
        .as_deref()
        .map(|sql| sql.split_whitespace().collect::<Vec<_>>().join(" "))
        != Some("CREATE UNIQUE INDEX fleet_login_single_row ON fleet_login ((1))".to_owned())
    {
        return Err(FleetStateOpenError::InvalidSchema);
    }
    Ok(())
}
