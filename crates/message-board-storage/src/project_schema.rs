//! Application schema identity is checked through the library's pinned read scope.
use crate::{
    ProjectStorageError,
    schema_fingerprint::{fingerprint_entries, schema_fingerprint},
};
use message_board::{SchemaFingerprint, SchemaRefusal};
use sqlx::{Executor, migrate::Migrator};
use sqlx_turso::Turso;
use std::collections::BTreeMap;

pub const PROJECT_SCHEMA_VERSION: u64 = 1;
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

pub fn expected_project_fingerprints()
-> Result<BTreeMap<u64, SchemaFingerprint>, ProjectStorageError> {
    let encoded: BTreeMap<String, SchemaFingerprint> =
        serde_json::from_str(include_str!("../schema_fingerprints/project.json"))?;
    let mut fingerprints = BTreeMap::new();
    for (version, fingerprint) in encoded {
        let version = version
            .parse::<u64>()
            .map_err(|_| ProjectStorageError::InvalidSchema)?;
        if version == 0
            || version > PROJECT_SCHEMA_VERSION
            || fingerprints.insert(version, fingerprint).is_some()
        {
            return Err(ProjectStorageError::InvalidSchema);
        }
    }
    if !fingerprints.contains_key(&PROJECT_SCHEMA_VERSION) {
        return Err(ProjectStorageError::InvalidSchema);
    }
    Ok(fingerprints)
}

pub async fn check_project_schema<TScope>(
    transaction: &mut TScope,
) -> Result<StoreSchemaState, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let version = sqlx_turso::query_scalar!(
        r#"SELECT MAX(version) AS "version?: i64" FROM _sqlx_migrations"#
    )
    .fetch_one(&mut *transaction)
    .await?
    .unwrap_or(0);
    let version = u64::try_from(version).map_err(|_| ProjectStorageError::InvalidSchema)?;
    if version > PROJECT_SCHEMA_VERSION {
        return Ok(StoreSchemaState::ReadOnly {
            refusal: SchemaRefusal::TooNew {
                store: version,
                supported: PROJECT_SCHEMA_VERSION,
            },
        });
    }
    let actual = schema_fingerprint(transaction).await?;
    let expected = if version == 0 {
        fingerprint_entries(Vec::new())?
    } else {
        expected_project_fingerprints()?
            .remove(&version)
            .ok_or(ProjectStorageError::InvalidSchema)?
    };
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
