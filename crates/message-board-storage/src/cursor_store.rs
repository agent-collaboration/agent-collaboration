//! Router-local signing keys serve live pages and retained legacy cursor translations.
use crate::{RouterLocalStorageError, router_local_database::RouterLocalDatabase};
use message_board::{
    ActivitySequence, BoardId, EpochNumber, FeedCursor, FeedPosition, ProjectId, ThreadId,
    WorkspaceId,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

static LEGACY_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./legacy_cursor_migrations");

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "scope",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum LegacyCursorScope {
    AllProjects {},
    Workspace(WorkspaceId),
    Board(BoardId),
    Project(ProjectId),
    Thread(ThreadId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyPosition {
    pub old: ActivitySequence,
    pub project: ProjectId,
    pub cursor: FeedCursor,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyScopeTranslation {
    pub scope: LegacyCursorScope,
    pub projects: Vec<ProjectId>,
}
pub struct CursorStore {
    database: RouterLocalDatabase,
}

impl CursorStore {
    pub async fn open(path: &Path) -> Result<Self, RouterLocalStorageError> {
        Ok(Self {
            database: RouterLocalDatabase::open(path, &LEGACY_MIGRATOR).await?,
        })
    }
    /// Initializes fresh installs without replacing an upgraded board's signing key.
    pub async fn get_or_install_key(
        &self,
        candidate: &CursorSigningKey,
    ) -> Result<CursorSigningKey, RouterLocalStorageError> {
        let _guard = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let stored = sqlx_turso::query_scalar!(r#"SELECT key_bytes AS "key_bytes!: Vec<u8>" FROM legacy_cursor_key WHERE singleton = 1"#).fetch_optional(&mut *transaction).await?;
        let key = match stored {
            Some(bytes) => CursorSigningKey::try_from(bytes)?,
            None => {
                let key = candidate.as_bytes();
                sqlx_turso::query!(
                    "INSERT INTO legacy_cursor_key(singleton, key_bytes) VALUES (1, ?)",
                    key
                )
                .execute(&mut *transaction)
                .await?;
                candidate.clone()
            }
        };
        transaction.commit().await?;
        Ok(key)
    }
    pub async fn install(
        &self,
        key: &CursorSigningKey,
        mapping: &[LegacyPosition],
        scopes: &[LegacyScopeTranslation],
    ) -> Result<(), RouterLocalStorageError> {
        let mut ordered = mapping.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|row| row.old);
        let mut positions = std::collections::BTreeMap::<ProjectId, u64>::new();
        let mut previous_old = ActivitySequence::ZERO;
        for row in ordered {
            let previous = positions.get(&row.project).copied().unwrap_or(0);
            if row.old <= previous_old
                || row.cursor.epoch.get() != 1
                || row.cursor.position.get() != previous + 1
            {
                return Err(RouterLocalStorageError::InvalidCursor);
            }
            positions.insert(row.project.clone(), row.cursor.position.get());
            previous_old = row.old;
        }
        for translation in scopes {
            let distinct = translation
                .projects
                .iter()
                .collect::<std::collections::HashSet<_>>();
            if distinct.len() != translation.projects.len() {
                return Err(RouterLocalStorageError::InvalidCursor);
            }
            match &translation.scope {
                LegacyCursorScope::Project(project)
                    if translation.projects.as_slice() != [project.clone()] =>
                {
                    return Err(RouterLocalStorageError::InvalidCursor);
                }
                LegacyCursorScope::Thread(_) if translation.projects.len() != 1 => {
                    return Err(RouterLocalStorageError::InvalidCursor);
                }
                _ => {}
            }
        }
        let _guard = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            let key = key.as_bytes();
            sqlx_turso::query!("INSERT INTO legacy_cursor_key(singleton, key_bytes) VALUES (1, ?)", key).execute(&mut *transaction).await?;
            for row in mapping {
                let old = i64::try_from(row.old.get()).map_err(|_| RouterLocalStorageError::InvalidCursor)?;
                let project = row.project.as_str(); let epoch = i64::try_from(row.cursor.epoch.get()).map_err(|_| RouterLocalStorageError::InvalidCursor)?; let position = i64::try_from(row.cursor.position.get()).map_err(|_| RouterLocalStorageError::InvalidCursor)?;
                sqlx_turso::query!("INSERT INTO legacy_position_mapping(old_position, project_id, epoch, position) VALUES (?, ?, ?, ?)", old, project, epoch, position).execute(&mut *transaction).await?;
            }
            for translation in scopes {
                let scope = serde_json::to_string(&translation.scope)?;
                sqlx_turso::query!("INSERT INTO legacy_cursor_scope_keys(scope) VALUES (?)", scope).execute(&mut *transaction).await?;
                for project in &translation.projects {
                    let project = project.as_str();
                    sqlx_turso::query!("INSERT INTO legacy_cursor_scopes(scope, project_id) VALUES (?, ?)", scope, project).execute(&mut *transaction).await?;
                }
            }
            Ok::<(), RouterLocalStorageError>(())
        }.await;
        match result {
            Ok(()) => {
                transaction.commit().await?;
                Ok(())
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }
    pub async fn key(&self) -> Result<Option<CursorSigningKey>, RouterLocalStorageError> {
        let key = sqlx_turso::query_scalar!(r#"SELECT key_bytes AS "key_bytes!: Vec<u8>" FROM legacy_cursor_key WHERE singleton = 1"#).fetch_optional(&self.database.pool).await?;
        key.map(CursorSigningKey::try_from).transpose()
    }
    pub async fn translate(
        &self,
        scope: &LegacyCursorScope,
        through: ActivitySequence,
    ) -> Result<Vec<(ProjectId, FeedCursor)>, RouterLocalStorageError> {
        let mut transaction = self.database.pool.begin().await?;
        let scope = serde_json::to_string(scope)?;
        let exists = sqlx_turso::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM legacy_cursor_scope_keys WHERE scope = ?) AS "exists!: i64""#, scope).fetch_one(&mut *transaction).await?;
        if exists != 1 {
            return Err(RouterLocalStorageError::InvalidCursor);
        }
        let projects = sqlx_turso::query_scalar!(r#"SELECT project_id AS "project_id!: String" FROM legacy_cursor_scopes WHERE scope = ? ORDER BY project_id"#, scope).fetch_all(&mut *transaction).await?;
        let through =
            i64::try_from(through.get()).map_err(|_| RouterLocalStorageError::InvalidCursor)?;
        let mut translated = Vec::new();
        for id in projects {
            let project = ProjectId::try_from(id.clone())
                .map_err(|_| RouterLocalStorageError::InvalidCursor)?;
            let row = sqlx_turso::query!(r#"SELECT epoch AS "epoch!: i64", position AS "position!: i64" FROM legacy_position_mapping WHERE project_id = ? AND old_position <= ? ORDER BY old_position DESC LIMIT 1"#, id, through).fetch_optional(&mut *transaction).await?;
            let cursor = match row {
                Some(row) => FeedCursor {
                    epoch: EpochNumber::try_from(
                        u64::try_from(row.epoch)
                            .map_err(|_| RouterLocalStorageError::InvalidCursor)?,
                    )
                    .map_err(|_| RouterLocalStorageError::InvalidCursor)?,
                    position: FeedPosition::try_from(
                        u64::try_from(row.position)
                            .map_err(|_| RouterLocalStorageError::InvalidCursor)?,
                    )
                    .map_err(|_| RouterLocalStorageError::InvalidCursor)?,
                },
                None => FeedCursor {
                    epoch: EpochNumber::try_from(1)
                        .map_err(|_| RouterLocalStorageError::InvalidCursor)?,
                    position: FeedPosition::ZERO,
                },
            };
            translated.push((project, cursor));
        }
        transaction.commit().await?;
        Ok(translated)
    }
    pub async fn close(self) {
        self.database.close().await;
    }
}
