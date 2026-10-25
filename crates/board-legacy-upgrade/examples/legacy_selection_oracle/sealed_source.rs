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
use rusqlite::{Connection, OpenFlags, Row};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SealedLegacyCopy {
    pub(crate) source_sha256: String,
    pub(crate) rows: LegacySourceRows,
}

pub(crate) fn read_sealed_copy(
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
    let sqlite_uri = immutable_sqlite_uri(&canonical_source)?;
    let connection = Connection::open_with_flags(
        sqlite_uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute_batch("PRAGMA query_only=ON; BEGIN;")?;
    let rows = read_legacy_rows(&connection)?;
    drop(connection);

    if has_sqlite_sidecar(&canonical_source)? || sha256_file(&canonical_source)? != source_sha256 {
        return Err(OracleError::LegacySourceChanged);
    }

    Ok(SealedLegacyCopy {
        source_sha256,
        rows,
    })
}

fn read_legacy_rows(connection: &Connection) -> Result<LegacySourceRows, OracleError> {
    let reader_keys = query_rows(
        connection,
        "SELECT identity_key FROM board_identities ORDER BY identity_key",
        |row| row.get::<_, String>(0),
    )?;
    let topic_ids = query_rows(
        connection,
        "SELECT topic_id FROM board_topics ORDER BY topic_id",
        |row| row.get::<_, String>(0),
    )?;
    let activity = read_legacy_activity_rows(connection)?;
    let main_starts = query_rows(
        connection,
        "SELECT reader_key,project_id,main_start FROM project_reader_state",
        |row| {
            Ok(LegacyMainStart {
                reader_key: row.get(0)?,
                project_id: row.get(1)?,
                main_start: row.get(2)?,
            })
        },
    )?;
    let topic_marks = query_rows(
        connection,
        "SELECT reader_key,topic_id,through_activity FROM topic_read_bookmarks",
        |row| {
            Ok(LegacyTopicMark {
                reader_key: row.get(0)?,
                topic_id: row.get(1)?,
                through_activity: row.get(2)?,
            })
        },
    )?;
    let thread_marks = query_rows(
        connection,
        "SELECT reader_key,root_id,through_activity FROM thread_read_bookmarks",
        |row| {
            Ok(LegacyThreadMark {
                reader_key: row.get(0)?,
                root_id: row.get(1)?,
                through_activity: row.get(2)?,
            })
        },
    )?;
    let raw_thread_watches = query_rows(
        connection,
        "SELECT reader_key,root_id,starts_after_activity,active FROM thread_watches",
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        },
    )?;
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

fn read_legacy_activity_rows(connection: &Connection) -> Result<Vec<LegacyActivity>, OracleError> {
    query_rows(
        connection,
        "SELECT activity_sequence,project_id,topic_id,root_id,kind,actor_key,message_id \
         FROM board_activity ORDER BY activity_sequence",
        |row| {
            let kind = row.get::<_, String>(4)?;
            Ok(LegacyActivity {
                activity_sequence: row.get(0)?,
                project_id: row.get(1)?,
                topic_id: row.get(2)?,
                root_id: row.get(3)?,
                kind: match kind.as_str() {
                    "mainMessageCreated" => LegacyActivityKind::MainMessageCreated,
                    "threadMessageCreated" => LegacyActivityKind::ThreadMessageCreated,
                    _ => LegacyActivityKind::Other,
                },
                actor_key: row.get(5)?,
                message_id: row.get(6)?,
            })
        },
    )
}

fn query_rows<T>(
    connection: &Connection,
    query: &str,
    decode_row: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, OracleError> {
    let mut statement = connection.prepare(query)?;
    let rows = statement.query_map([], decode_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
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

fn immutable_sqlite_uri(source_path: &Path) -> Result<String, OracleError> {
    let path = source_path
        .to_str()
        .ok_or(OracleError::UnsupportedLegacySourcePath)?;
    let mut encoded_path = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            encoded_path.push(char::from(byte));
        } else {
            encoded_path.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(format!("file:{encoded_path}?mode=ro&immutable=1"))
}
