//! SQLite is used only to read the retired format; all destination writes use Turso.
use crate::{UpgradeError, import_error::invalid, legacy_rows::*};
use chrono::{DateTime, Utc};
use message_board::*;
use rusqlite::{Connection, OpenFlags};
use serde::de::DeserializeOwned;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) async fn read_legacy(path: PathBuf) -> Result<LegacyStore, UpgradeError> {
    tokio::task::spawn_blocking(move || read_snapshot(&path)).await?
}

pub(crate) fn refuse_open_file(path: &Path) -> Result<(), UpgradeError> {
    let result = Command::new("lsof")
        .args(["-t", "--"])
        .arg(path)
        .output()
        .map_err(|error| UpgradeError::OpenFileInspection(error.to_string()))?;
    if !result.status.success() && result.status.code() != Some(1) {
        return Err(UpgradeError::OpenFileInspection(
            String::from_utf8_lossy(&result.stderr).into_owned(),
        ));
    }
    for pid in String::from_utf8_lossy(&result.stdout).lines() {
        let pid = pid
            .parse::<u32>()
            .map_err(|error| UpgradeError::OpenFileInspection(error.to_string()))?;
        if pid != std::process::id() {
            return Err(UpgradeError::SourceInUse(path.to_owned()));
        }
    }
    Ok(())
}

// Identifiers and expressions are private constants below, never supplied by a caller.
// Named JSON fields decouple source column order from the checked domain constructors.
fn rows<T: DeserializeOwned>(
    connection: &Connection,
    table: &str,
    fields: &[(&str, &str)],
    order: &str,
) -> Result<Vec<T>, UpgradeError> {
    let columns = fields
        .iter()
        .map(|(name, expression)| format!("'{name}',{expression}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut query = connection.prepare(&format!(
        "SELECT json_object({columns}) AS payload FROM {table} ORDER BY {order}"
    ))?;
    let mut cursor = query.query([])?;
    let mut result = Vec::new();
    while let Some(row) = cursor.next()? {
        let payload: String = row.get("payload")?;
        result.push(crate::import_error::decode(&payload, table)?);
    }
    Ok(result)
}

macro_rules! fields { ($($field:literal),* $(,)?) => { &[$(($field,$field)),*] }; }

// Decode the identity separately so any invalid topic field, including an
// oversized description, has a typed refusal naming the affected topic.
fn read_projects(connection: &Connection) -> Result<Vec<LegacyProject>, UpgradeError> {
    let mut statement = connection.prepare(
        "SELECT topic_id,json_object('topic_id',topic_id,'board_id',board_id,'name',name,'description',description) AS payload FROM board_topics ORDER BY topic_id",
    )?;
    let mut rows = statement.query([])?;
    let mut projects = Vec::new();
    while let Some(row) = rows.next()? {
        let topic = ProjectId::try_from(row.get::<_, String>("topic_id")?)
            .map_err(|error| invalid("topic_id", error))?;
        let payload: String = row.get("payload")?;
        projects.push(serde_json::from_str(&payload).map_err(|error| {
            UpgradeError::InvalidLegacyTopic {
                topic,
                detail: error.to_string(),
            }
        })?);
    }
    Ok(projects)
}

fn read_snapshot(path: &Path) -> Result<LegacyStore, UpgradeError> {
    let path = path.canonicalize()?;
    refuse_open_file(&path)?;
    let connection = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute_batch("PRAGMA query_only=ON; BEGIN;")?;
    crate::legacy_schema::validate_supported_schema(&connection)?;
    let workspaces = rows(
        &connection,
        "board_projects",
        fields!["project_id", "name", "description"],
        "project_id",
    )?;
    let boards = rows(
        &connection,
        "project_boards",
        fields!["board_id", "project_id", "name", "description", "state"],
        "board_id",
    )?;
    let projects = read_projects(&connection)?;
    let identities: Vec<LegacyIdentity> = rows(
        &connection,
        "board_identities",
        fields![
            "identity_key",
            "kind",
            "service_id",
            "endpoint_id",
            "session_id",
            "human_id"
        ],
        "identity_key",
    )?;
    let mut actors = BTreeMap::new();
    for row in identities {
        let actor = match (
            row.kind.as_str(),
            row.service_id,
            row.endpoint_id,
            row.session_id,
            row.human_id,
        ) {
            ("session", Some(service_id), Some(endpoint_id), Some(session_id), None) => {
                Identity::Session {
                    session: SessionRef {
                        endpoint: SessionEndpointRef {
                            service_id,
                            endpoint_id,
                        },
                        session_id,
                    },
                }
            }
            ("human", None, None, None, Some(human_id)) => Identity::Human { human_id },
            _ => {
                return Err(invalid(
                    "board_identities",
                    "identity fields do not match kind",
                ));
            }
        };
        let canonical = match &actor {
            Identity::Human { human_id } => format!("human:{}", human_id.as_str()),
            Identity::Session { session } => format!(
                "session:{}:{}:{}",
                session.endpoint.service_id.as_str(),
                session.endpoint.endpoint_id.as_str(),
                session.session_id.as_str()
            ),
        };
        if canonical != row.identity_key {
            return Err(invalid(
                "identity_key",
                "stored key disagrees with identity",
            ));
        }
        actors.insert(row.identity_key, actor);
    }
    let threads: Vec<LegacyThread> = rows(
        &connection,
        "board_threads",
        fields!["root_id", "state"],
        "root_id",
    )?;
    let threads = threads
        .into_iter()
        .map(|thread| {
            (
                thread.root_id,
                match thread.state {
                    LegacyThreadState::Unresolved => ThreadState::Open,
                    LegacyThreadState::Resolved => ThreadState::Resolved,
                },
            )
        })
        .collect();
    let posts: Vec<LegacyPost> = rows(
        &connection,
        "board_messages",
        fields![
            "message_id",
            "topic_id",
            "board_id",
            "root_id",
            "actor_key",
            "acting_for_key",
            "text",
            "posted_from_activity"
        ],
        "message_id",
    )?;
    let activity = rows(
        &connection,
        "board_activity",
        fields![
            "activity_sequence",
            "topic_id",
            "root_id",
            "kind",
            "actor_key",
            "message_id",
            "participant_key",
            "participant_role",
            "replaced_participant_key"
        ],
        "activity_sequence",
    )?;
    let participants = rows(
        &connection,
        "thread_participants",
        fields![
            "reader_key",
            "root_id",
            "role",
            "note",
            "joined_at_activity",
            "last_seen_activity",
            "closed_at_activity",
            "closed_reason",
            "replaced_by"
        ],
        "root_id,reader_key",
    )?;
    let project_marks = rows(
        &connection,
        "topic_read_bookmarks",
        &[
            ("reader_key", "reader_key"),
            ("scope_id", "topic_id"),
            ("through_activity", "through_activity"),
        ],
        "topic_id,reader_key",
    )?;
    let thread_marks = rows(
        &connection,
        "thread_read_bookmarks",
        &[
            ("reader_key", "reader_key"),
            ("scope_id", "root_id"),
            ("through_activity", "through_activity"),
        ],
        "root_id,reader_key",
    )?;
    let project_watches = rows(
        &connection,
        "topic_watches",
        &[
            ("reader_key", "reader_key"),
            ("scope_id", "topic_id"),
            ("starts_after_activity", "starts_after_activity"),
            (
                "active",
                "json(CASE active WHEN 0 THEN 'false' WHEN 1 THEN 'true' ELSE 'null' END)",
            ),
        ],
        "topic_id,reader_key",
    )?;
    let thread_watches = rows(
        &connection,
        "thread_watches",
        &[
            ("reader_key", "reader_key"),
            ("scope_id", "root_id"),
            ("starts_after_activity", "starts_after_activity"),
            (
                "active",
                "json(CASE active WHEN 0 THEN 'false' WHEN 1 THEN 'true' ELSE 'null' END)",
            ),
        ],
        "root_id,reader_key",
    )?;
    let tracking = rows(
        &connection,
        "project_reader_state",
        fields!["reader_key", "project_id", "main_start"],
        "project_id,reader_key",
    )?;
    let subscriptions = rows(
        &connection,
        "thread_subscriptions",
        fields![
            "reader_key",
            "scope_kind",
            "scope_id",
            "mode",
            "when_idle",
            "quiet_seconds",
            "cap_seconds",
            "lifetime_seconds",
            "renewed_at",
            "expires_at",
            "state",
            "end_reason",
            "ended_at",
            "generation"
        ],
        "scope_kind,scope_id,reader_key",
    )?;
    let repositories = rows(
        &connection,
        "project_repositories",
        fields![
            "project_id",
            "kind",
            "origin",
            "service_id",
            "common_directory"
        ],
        "project_id,repository_key",
    )?;
    #[derive(serde::Deserialize)]
    struct Reference {
        source_id: PostId,
        ordinal: u64,
        kind: String,
        target_message_id: Option<PostId>,
        target_root_id: Option<ThreadId>,
    }
    let reference_rows: Vec<Reference> = rows(
        &connection,
        "message_references",
        fields![
            "source_id",
            "ordinal",
            "kind",
            "target_message_id",
            "target_root_id"
        ],
        "source_id,ordinal",
    )?;
    let mut references: BTreeMap<PostId, Vec<ReferenceTarget>> = BTreeMap::new();
    for row in reference_rows {
        let count = references.get(&row.source_id).map_or(0, Vec::len);
        if row.ordinal
            != u64::try_from(count).map_err(|error| invalid("reference ordinal", error))?
        {
            return Err(invalid(
                "reference ordinal",
                "ordered reference positions are not dense",
            ));
        }
        let reference = match (row.kind.as_str(), row.target_message_id, row.target_root_id) {
            ("message", Some(post_id), None) => ReferenceTarget::Post { post_id },
            ("thread", None, Some(thread_id)) => ReferenceTarget::Thread { thread_id },
            _ => return Err(invalid("message_references", "target does not match kind")),
        };
        references.entry(row.source_id).or_default().push(reference);
    }
    let last_recorded_at = last_recorded_time(&connection)?;
    refuse_open_file(&path)?;
    connection.execute_batch("ROLLBACK")?;
    Ok(LegacyStore {
        workspaces,
        boards,
        projects,
        actors,
        posts: posts
            .into_iter()
            .map(|post| (post.message_id.clone(), post))
            .collect(),
        threads,
        activity,
        participants,
        project_marks,
        thread_marks,
        project_watches,
        thread_watches,
        tracking,
        subscriptions,
        repositories,
        references,
        last_recorded_at,
    })
}

fn last_recorded_time(connection: &Connection) -> Result<Option<DateTime<Utc>>, UpgradeError> {
    let mut last = None;
    for sql in [
        "SELECT renewed_at AS recorded_at FROM thread_subscriptions UNION ALL SELECT ended_at FROM thread_subscriptions WHERE ended_at IS NOT NULL",
        "SELECT opened_at AS recorded_at FROM subscription_windows UNION ALL SELECT last_arrival_at FROM subscription_windows UNION ALL SELECT held_since FROM subscription_windows WHERE held_since IS NOT NULL",
    ] {
        let mut statement = connection.prepare(sql)?;
        let mut cursor = statement.query([])?;
        while let Some(row) = cursor.next()? {
            let text: String = row.get("recorded_at")?;
            let at = text
                .parse::<DateTime<Utc>>()
                .map_err(|error| invalid("recorded_at", error))?;
            last = Some(last.map_or(at, |previous: DateTime<Utc>| previous.max(at)));
        }
    }
    let mut statement = connection.prepare("SELECT last_post_at_ms FROM actor_board_cooldowns")?;
    let mut cursor = statement.query([])?;
    while let Some(row) = cursor.next()? {
        let value: i64 = row.get("last_post_at_ms")?;
        let at = DateTime::from_timestamp_millis(value)
            .ok_or_else(|| invalid("last_post_at_ms", "out of range"))?;
        last = Some(last.map_or(at, |previous| previous.max(at)));
    }
    Ok(last)
}
