//! Optional local UI preferences. Consumers own the meaning of opaque text values.
use sqlx::{
    Connection, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::time::Duration;
use std::{error::Error, fmt, path::Path};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone, Debug)]
pub struct UiPreferenceStore {
    pool: Option<SqlitePool>,
}

struct StoredUiPreference {
    preference_id: String,
    value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiPreferenceSaveError;

impl fmt::Display for UiPreferenceSaveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UI preference could not be saved")
    }
}
impl Error for UiPreferenceSaveError {}

impl UiPreferenceStore {
    /// Unavailable or damaged preferences are optional; opening never replaces their file.
    pub async fn open_and_migrate(path: &Path) -> Self {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Delete)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(Duration::from_millis(250));
        let Ok(pool) = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
        else {
            return Self { pool: None };
        };
        if initialize_schema(&pool).await.is_err() {
            pool.close().await;
            return Self { pool: None };
        }
        Self { pool: Some(pool) }
    }

    pub async fn load(&self, screen: &str, setting: &str) -> Option<String> {
        let pool = self.pool.as_ref()?;
        let row = sqlx::query_as!(
            StoredUiPreference,
            "SELECT preference_id, value FROM ui_preferences WHERE screen = ? AND setting = ?",
            screen,
            setting
        )
        .fetch_optional(pool)
        .await
        .ok()??;
        let identity = uuid::Uuid::parse_str(&row.preference_id).ok()?;
        (identity.get_version_num() == 7 && identity.get_variant() == uuid::Variant::RFC4122)
            .then_some(row.value)
    }

    pub async fn save(
        &self,
        screen: &str,
        setting: &str,
        value: &str,
    ) -> Result<(), UiPreferenceSaveError> {
        let pool = self.pool.as_ref().ok_or(UiPreferenceSaveError)?;
        let preference_id = uuid::Uuid::now_v7().to_string();
        sqlx::query!("INSERT INTO ui_preferences (preference_id, screen, setting, value) VALUES (?, ?, ?, ?) ON CONFLICT(screen, setting) DO UPDATE SET value = excluded.value",
            preference_id, screen, setting, value).execute(pool).await.map_err(|_| UiPreferenceSaveError)?;
        Ok(())
    }

    pub async fn close(self) {
        if let Some(pool) = self.pool {
            pool.close().await;
        }
    }
}

async fn initialize_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut connection = pool.acquire().await?;
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    let has_history: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations')").fetch_one(&mut *transaction).await?;
    if !has_history {
        let has_unregistered_objects: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name NOT LIKE 'sqlite_%')",
        )
        .fetch_one(&mut *transaction)
        .await?;
        if has_unregistered_objects {
            return Err(sqlx::Error::Protocol(
                "unrecognized UI preference database".to_owned(),
            ));
        }
    }
    MIGRATOR
        .run_direct(None, &mut *transaction, false)
        .await
        .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
    transaction.commit().await
}

#[cfg(test)]
mod sqlite_preferences_test;
