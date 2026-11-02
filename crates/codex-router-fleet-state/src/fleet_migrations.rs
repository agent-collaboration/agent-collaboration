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
    validate_registration_schema(connection).await
}

async fn validate_registration_schema(
    connection: &mut SqliteConnection,
) -> Result<(), FleetStateOpenError> {
    let columns = sqlx::query("PRAGMA table_info(fleet_session_registrations)")
        .fetch_all(&mut *connection)
        .await
        .map_err(|_| FleetStateOpenError::InvalidSchema)?;
    let names = [
        "registration_id",
        "operation_id",
        "installation_id",
        "binding_address",
        "session_service_id",
        "session_endpoint_id",
        "session_id",
        "lifecycle_json",
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
        let expected_required = !expected.starts_with("session_");
        if name != expected
            || kind != "TEXT"
            || required != expected_required
            || primary != i64::from(expected == "registration_id")
        {
            return Err(FleetStateOpenError::InvalidSchema);
        }
    }
    let indexes = sqlx::query("PRAGMA index_list(fleet_session_registrations)")
        .fetch_all(&mut *connection)
        .await
        .map_err(|_| FleetStateOpenError::InvalidSchema)?;
    for expected in [
        "fleet_session_registration_operation",
        "fleet_session_registration_binding",
        "fleet_session_registration_conversation",
    ] {
        let mut found = false;
        for index in &indexes {
            let name: String = index
                .try_get("name")
                .map_err(|_| FleetStateOpenError::InvalidSchema)?;
            if name == expected {
                let unique: bool = index
                    .try_get("unique")
                    .map_err(|_| FleetStateOpenError::InvalidSchema)?;
                let partial: bool = index
                    .try_get("partial")
                    .map_err(|_| FleetStateOpenError::InvalidSchema)?;
                found = unique && !partial;
            }
        }
        if !found {
            return Err(FleetStateOpenError::InvalidSchema);
        }
    }
    for (query, expected) in [
        (
            "PRAGMA index_info(fleet_session_registration_operation)",
            &["operation_id"][..],
        ),
        (
            "PRAGMA index_info(fleet_session_registration_binding)",
            &["binding_address"][..],
        ),
        (
            "PRAGMA index_info(fleet_session_registration_conversation)",
            &["session_service_id", "session_endpoint_id", "session_id"][..],
        ),
    ] {
        let parts = sqlx::query(query)
            .fetch_all(&mut *connection)
            .await
            .map_err(|_| FleetStateOpenError::InvalidSchema)?;
        if parts.len() != expected.len() {
            return Err(FleetStateOpenError::InvalidSchema);
        }
        for (part, expected) in parts.iter().zip(expected) {
            let name: String = part
                .try_get("name")
                .map_err(|_| FleetStateOpenError::InvalidSchema)?;
            if name != *expected {
                return Err(FleetStateOpenError::InvalidSchema);
            }
        }
    }
    Ok(())
}
