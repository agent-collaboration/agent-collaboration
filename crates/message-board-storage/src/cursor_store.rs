//! Router-local pagination keys belong only to the new database format.
use crate::{RouterLocalStorageError, router_local_database::RouterLocalDatabase};
use sqlx::Executor;
use sqlx_turso::Turso;
use std::path::Path;
use uuid::{Uuid, Variant};

static CURSOR_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./cursor_migrations");

#[derive(Clone, Eq, PartialEq)]
pub struct CursorSigningKey(Vec<u8>);
impl std::fmt::Debug for CursorSigningKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CursorSigningKey([redacted])")
    }
}
impl TryFrom<Vec<u8>> for CursorSigningKey {
    type Error = RouterLocalStorageError;
    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        if value.len() != 32 {
            return Err(RouterLocalStorageError::InvalidCursor);
        }
        Ok(Self(value))
    }
}
impl CursorSigningKey {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

struct CursorSigningKeyId(String);

impl CursorSigningKeyId {
    fn generate() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CursorSigningKeyId {
    type Error = RouterLocalStorageError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let invalid = || RouterLocalStorageError::InvalidRecordKey {
            field: "cursor_signing_key.key_id",
        };
        let parsed = Uuid::parse_str(&value).map_err(|_| invalid())?;
        if parsed.get_version_num() != 7
            || parsed.get_variant() != Variant::RFC4122
            || parsed.hyphenated().to_string() != value
        {
            return Err(invalid());
        }
        Ok(Self(value))
    }
}

struct StoredCursorSigningKey {
    key_id: Option<String>,
    key_bytes: Vec<u8>,
}

async fn load_stored_signing_key<'executor>(
    executor: impl Executor<'executor, Database = Turso>,
) -> Result<Option<CursorSigningKey>, RouterLocalStorageError> {
    let mut rows = sqlx_turso::query_as!(StoredCursorSigningKey, r#"SELECT key_id AS "key_id?: String", key_bytes AS "key_bytes!: Vec<u8>" FROM cursor_signing_key LIMIT 2"#)
        .fetch_all(executor)
        .await?;
    if rows.len() > 1 {
        return Err(RouterLocalStorageError::InvalidRecord);
    }
    let Some(row) = rows.pop() else {
        return Ok(None);
    };
    let key_id = row
        .key_id
        .ok_or(RouterLocalStorageError::InvalidRecordKey {
            field: "cursor_signing_key.key_id",
        })?;
    CursorSigningKeyId::try_from(key_id)?;
    Ok(Some(CursorSigningKey::try_from(row.key_bytes)?))
}

pub struct CursorStore {
    database: RouterLocalDatabase,
}

impl CursorStore {
    pub async fn open(path: &Path) -> Result<Self, RouterLocalStorageError> {
        Ok(Self {
            database: RouterLocalDatabase::open(path, &CURSOR_MIGRATOR).await?,
        })
    }
    /// Reuses this format's key after restart without replacing it with the candidate.
    pub async fn get_or_install_key(
        &self,
        candidate: &CursorSigningKey,
    ) -> Result<CursorSigningKey, RouterLocalStorageError> {
        let _guard = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let stored = load_stored_signing_key(&mut *transaction).await?;
        let key = match stored {
            Some(key) => key,
            None => {
                let key_identity = CursorSigningKeyId::generate();
                let key_id = key_identity.as_str();
                let key_bytes = candidate.as_bytes();
                sqlx_turso::query!(
                    "INSERT INTO cursor_signing_key(key_id, key_bytes) VALUES (?, ?)",
                    key_id,
                    key_bytes
                )
                .execute(&mut *transaction)
                .await?;
                candidate.clone()
            }
        };
        transaction.commit().await?;
        Ok(key)
    }
    pub async fn key(&self) -> Result<Option<CursorSigningKey>, RouterLocalStorageError> {
        load_stored_signing_key(&self.database.pool).await
    }
    pub async fn close(self) {
        self.database.close().await;
    }
}
