#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LegacyActivity {
    pub(crate) activity_sequence: i64,
    pub(crate) project_id: String,
    pub(crate) topic_id: String,
    pub(crate) root_id: Option<String>,
    pub(crate) kind: LegacyActivityKind,
    pub(crate) actor_key: String,
    pub(crate) message_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LegacyActivityKind {
    MainMessageCreated,
    ThreadMessageCreated,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LegacyMainStart {
    pub(crate) reader_key: String,
    pub(crate) project_id: String,
    pub(crate) main_start: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LegacyTopicMark {
    pub(crate) reader_key: String,
    pub(crate) topic_id: String,
    pub(crate) through_activity: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LegacyThreadMark {
    pub(crate) reader_key: String,
    pub(crate) root_id: String,
    pub(crate) through_activity: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LegacyThreadWatch {
    pub(crate) reader_key: String,
    pub(crate) root_id: String,
    pub(crate) starts_after_activity: i64,
    pub(crate) active: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LegacySourceRows {
    pub(crate) activity: Vec<LegacyActivity>,
    pub(crate) main_starts: Vec<LegacyMainStart>,
    pub(crate) topic_marks: Vec<LegacyTopicMark>,
    pub(crate) thread_marks: Vec<LegacyThreadMark>,
    pub(crate) thread_watches: Vec<LegacyThreadWatch>,
    pub(crate) reader_keys: Vec<String>,
    pub(crate) topic_ids: Vec<String>,
}
use crate::{oracle_errors::OracleError, oracle_hashes::sha256_file};
use futures_util::TryStreamExt;
use sqlx::{
    Connection, Row, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteRow},
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SealedLegacyCopy {
    pub(crate) source_sha256: String,
    pub(crate) rows: LegacySourceRows,
}

pub(crate) async fn read_sealed_copy(
    source_path: &Path,
    copy_root: &Path,
) -> Result<SealedLegacyCopy, OracleError> {
    let canonical_source = source_path.canonicalize()?;
    let canonical_root = copy_root.canonicalize()?;
    let source_metadata = canonical_source.metadata()?;
    if !source_metadata.is_file()
        || canonical_source == canonical_root
        || !canonical_source.starts_with(&canonical_root)
    {
        return Err(OracleError::InvalidLegacySourcePath);
    }
    if is_normal_codex_state_path(&canonical_source) {
        return Err(OracleError::NormalStateSourcePath);
    }
    if has_sqlite_sidecar(&canonical_source)? {
        return Err(OracleError::UnsealedLegacySource);
    }

    let source_sha256 = sha256_file(&canonical_source)?;
    canonical_source
        .to_str()
        .ok_or(OracleError::UnsupportedLegacySourcePath)?;
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&canonical_source)
            .read_only(true)
            .create_if_missing(false)
            .immutable(true)
            .foreign_keys(false)
            .pragma("query_only", "ON"),
    )
    .await?;
    let source_result = async {
        sqlx::raw_sql("BEGIN;").execute(&mut connection).await?;
        read_legacy_rows(&mut connection).await
    }
    .await;
    let rollback_result = sqlx::raw_sql("ROLLBACK;").execute(&mut connection).await;
    let close_result = connection.close().await;
    let rows = source_result?;
    rollback_result?;
    close_result?;

    if has_sqlite_sidecar(&canonical_source)? || sha256_file(&canonical_source)? != source_sha256 {
        return Err(OracleError::LegacySourceChanged);
    }

    Ok(SealedLegacyCopy {
        source_sha256,
        rows,
    })
}

async fn read_legacy_rows(
    connection: &mut SqliteConnection,
) -> Result<LegacySourceRows, OracleError> {
    let reader_keys = query_rows(
        connection,
        "SELECT identity_key FROM board_identities ORDER BY identity_key",
        |row| row.try_get::<String, _>("identity_key"),
    )
    .await?;
    let topic_ids = query_rows(
        connection,
        "SELECT topic_id FROM board_topics ORDER BY topic_id",
        |row| row.try_get::<String, _>("topic_id"),
    )
    .await?;
    let activity = read_legacy_activity_rows(connection).await?;
    let main_starts = query_rows(
        connection,
        "SELECT reader_key,project_id,main_start FROM project_reader_state",
        |row| {
            Ok(LegacyMainStart {
                reader_key: row.try_get("reader_key")?,
                project_id: row.try_get("project_id")?,
                main_start: row.try_get("main_start")?,
            })
        },
    )
    .await?;
    let topic_marks = query_rows(
        connection,
        "SELECT reader_key,topic_id,through_activity FROM topic_read_bookmarks",
        |row| {
            Ok(LegacyTopicMark {
                reader_key: row.try_get("reader_key")?,
                topic_id: row.try_get("topic_id")?,
                through_activity: row.try_get("through_activity")?,
            })
        },
    )
    .await?;
    let thread_marks = query_rows(
        connection,
        "SELECT reader_key,root_id,through_activity FROM thread_read_bookmarks",
        |row| {
            Ok(LegacyThreadMark {
                reader_key: row.try_get("reader_key")?,
                root_id: row.try_get("root_id")?,
                through_activity: row.try_get("through_activity")?,
            })
        },
    )
    .await?;
    let raw_thread_watches = query_rows(
        connection,
        "SELECT reader_key,root_id,starts_after_activity,active FROM thread_watches",
        |row| {
            Ok((
                row.try_get::<String, _>("reader_key")?,
                row.try_get::<String, _>("root_id")?,
                row.try_get::<i64, _>("starts_after_activity")?,
                row.try_get::<i64, _>("active")?,
            ))
        },
    )
    .await?;
    let thread_watches = raw_thread_watches
        .into_iter()
        .map(
            |(reader_key, root_id, starts_after_activity, active)| -> Result<_, OracleError> {
                let active = match active {
                    0 => false,
                    1 => true,
                    invalid => return Err(OracleError::InvalidWatchActiveValue(invalid)),
                };
                Ok(LegacyThreadWatch {
                    reader_key,
                    root_id,
                    starts_after_activity,
                    active,
                })
            },
        )
        .collect::<Result<Vec<_>, _>>()?;

    Ok(LegacySourceRows {
        activity,
        main_starts,
        topic_marks,
        thread_marks,
        thread_watches,
        reader_keys,
        topic_ids,
    })
}

async fn read_legacy_activity_rows(
    connection: &mut SqliteConnection,
) -> Result<Vec<LegacyActivity>, OracleError> {
    query_rows(
        connection,
        "SELECT activity_sequence,project_id,topic_id,root_id,kind,actor_key,message_id \
         FROM board_activity ORDER BY activity_sequence",
        |row| {
            let kind = row.try_get::<String, _>("kind")?;
            Ok(LegacyActivity {
                activity_sequence: row.try_get("activity_sequence")?,
                project_id: row.try_get("project_id")?,
                topic_id: row.try_get("topic_id")?,
                root_id: row.try_get("root_id")?,
                kind: match kind.as_str() {
                    "mainMessageCreated" => LegacyActivityKind::MainMessageCreated,
                    "threadMessageCreated" => LegacyActivityKind::ThreadMessageCreated,
                    _ => LegacyActivityKind::Other,
                },
                actor_key: row.try_get("actor_key")?,
                message_id: row.try_get("message_id")?,
            })
        },
    )
    .await
}

async fn query_rows<T>(
    connection: &mut SqliteConnection,
    query: &str,
    mut decode_row: impl FnMut(&SqliteRow) -> Result<T, sqlx::Error>,
) -> Result<Vec<T>, OracleError> {
    let mut cursor = sqlx::query(sqlx::AssertSqlSafe(query)).fetch(connection);
    let mut rows = Vec::new();
    while let Some(row) = cursor.try_next().await? {
        rows.push(decode_row(&row)?);
    }
    Ok(rows)
}

fn is_normal_codex_state_path(path: &Path) -> bool {
    path.ancestors()
        .skip(1)
        .filter_map(Path::file_name)
        .any(|ancestor_name| {
            let ancestor_name = ancestor_name.to_string_lossy();
            ancestor_name.starts_with(".codex") || ancestor_name.starts_with(".codex-router")
        })
}

fn has_sqlite_sidecar(source_path: &Path) -> Result<bool, OracleError> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = OsString::from(source_path.as_os_str());
        sidecar.push(suffix);
        if PathBuf::from(sidecar).try_exists()? {
            return Ok(true);
        }
    }
    Ok(false)
}
