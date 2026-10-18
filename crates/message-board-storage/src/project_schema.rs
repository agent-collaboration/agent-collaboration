use message_board::{SchemaFingerprint, SchemaRefusal};
use sqlx::{Connection, migrate::Migrator};
use sqlx_turso::TursoConnection;

use crate::{
    ProjectStorageError,
    schema_fingerprint::{fingerprint_entries, schema_fingerprint},
};

pub const PROJECT_SCHEMA_VERSION: u64 = 202610150008;
pub static PROJECT_MIGRATOR: Migrator = sqlx::migrate!("./project_migrations");

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoreSchemaState {
    Ready {
        version: u64,
        fingerprint: SchemaFingerprint,
    },
    ReadOnly {
        refusal: SchemaRefusal,
    },
}

fn expected_fingerprint(version: u64) -> Result<SchemaFingerprint, ProjectStorageError> {
    if version == 0 {
        return fingerprint_entries(Vec::new());
    }
    let fingerprints: std::collections::BTreeMap<String, SchemaFingerprint> =
        serde_json::from_str(include_str!("../schema_fingerprints/project.json"))?;
    fingerprints
        .get(&version.to_string())
        .cloned()
        .ok_or(ProjectStorageError::InvalidSchema)
}

pub async fn check_schema(
    connection: &mut TursoConnection,
) -> Result<StoreSchemaState, ProjectStorageError> {
    let has_history = sqlx_turso::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = '_sqlx_migrations') AS "exists!: i64""#).fetch_one(&mut *connection).await?;
    let version = if has_history != 0 {
        let version = sqlx_turso::query_scalar!(
            r#"SELECT MAX(version) AS "version?: i64" FROM _sqlx_migrations"#
        )
        .fetch_one(&mut *connection)
        .await?
        .unwrap_or(0);
        u64::try_from(version).map_err(|_| ProjectStorageError::InvalidSchema)?
    } else {
        0
    };
    if version > PROJECT_SCHEMA_VERSION {
        return Ok(StoreSchemaState::ReadOnly {
            refusal: SchemaRefusal::TooNew {
                store: version,
                supported: PROJECT_SCHEMA_VERSION,
            },
        });
    }
    let actual = schema_fingerprint(connection).await?;
    let expected = expected_fingerprint(version)?;
    if actual != expected {
        return Ok(StoreSchemaState::ReadOnly {
            refusal: SchemaRefusal::Mismatch {
                version,
                expected,
                actual,
            },
        });
    }
    Ok(StoreSchemaState::Ready {
        version,
        fingerprint: actual,
    })
}

pub async fn migrate_project_schema(
    connection: &mut TursoConnection,
) -> Result<StoreSchemaState, ProjectStorageError> {
    let initial = check_schema(connection).await?;
    match initial {
        StoreSchemaState::ReadOnly { .. } => return Ok(initial),
        StoreSchemaState::Ready { version, .. } if version == PROJECT_SCHEMA_VERSION => {
            return Ok(initial);
        }
        StoreSchemaState::Ready { .. } => {}
    }
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *connection)
        .await?;
    let result = migrate_owned(connection).await;
    let enforcement = sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *connection)
        .await;
    enforcement?;
    result
}

async fn migrate_owned(
    connection: &mut TursoConnection,
) -> Result<StoreSchemaState, ProjectStorageError> {
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    let result = async {
        PROJECT_MIGRATOR
            .run_direct(None, &mut *transaction, false)
            .await
            .map_err(|_| ProjectStorageError::InvalidSchema)?;
        if !sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *transaction)
            .await?
            .is_empty()
        {
            return Err(ProjectStorageError::InvalidSchema);
        }
        let state = check_schema(&mut transaction).await?;
        if let StoreSchemaState::ReadOnly { refusal } = state {
            return Err(ProjectStorageError::SchemaNotReady(refusal));
        }
        Ok(state)
    }
    .await;
    match result {
        Ok(state) => {
            transaction.commit().await?;
            Ok(state)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}
