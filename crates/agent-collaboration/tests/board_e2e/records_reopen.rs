//! BR07/08/10/11 records overlay: PD2 §3a keys/schema and VD §2 owning reopen.
use super::fixture::BoardFixture;
use codex_router_test_support::FixtureResult;
use sqlx_turso::sqlx::{AssertSqlSafe, ConnectOptions, Connection, Row};
use std::{collections::BTreeMap, path::PathBuf};

const KEYS: &[(&str, &str)] = &[
    ("project_state", "project_id"),
    ("workstreams", "workstream_id"),
    ("tasks", "task_id"),
    ("task_links", "link_id"),
    ("task_link_events", "event_id"),
    ("subtasks", "subtask_id"),
    ("actors", "actor_id"),
    ("threads", "thread_id"),
    ("thread_refilings", "refiling_id"),
    ("posts", "post_id"),
    ("post_references", "reference_id"),
    ("thread_participation", "participation_id"),
    ("role_assignments", "assignment_id"),
    ("owner_grants", "grant_id"),
    ("stand_down_acknowledgements", "acknowledgement_id"),
    ("work_trail_rows", "row_id"),
    ("work_trail_tags", "tag_id"),
    ("work_trail_links", "link_id"),
    ("moves", "move_id"),
    ("facts", "fact_id"),
    ("plan_revisions", "revision_id"),
    ("read_marks", "mark_id"),
    ("thread_watches", "watch_id"),
    ("project_watches", "watch_id"),
    ("subscription_memberships", "membership_id"),
    ("notify_intents", "intent_id"),
    ("mail_handoffs", "item_id"),
    ("handoff_frontier", "frontier_id"),
    ("request_decisions", "request_id"),
    ("epoch_lines", "line_id"),
    ("project_feed", "entry_id"),
];
const BUSINESS_KEYS: &[(&str, &[&str])] = &[
    ("task_links", &["from_task", "link_kind", "to_task"]),
    ("actors", &["canonical_identity"]),
    ("post_references", &["source_post", "place"]),
    (
        "thread_participation",
        &["actor_id", "thread_id", "joined_epoch", "joined_position"],
    ),
    ("role_assignments", &["owner_grant"]),
    ("work_trail_tags", &["row_id", "place"]),
    ("work_trail_links", &["row_id", "place"]),
    ("plan_revisions", &["content_hash"]),
    ("read_marks", &["reader_actor", "scope_kind", "scope_id"]),
    ("thread_watches", &["reader_actor", "thread_id"]),
    ("project_watches", &["reader_actor"]),
    (
        "subscription_memberships",
        &["reader_actor", "scope_kind", "scope_id", "generation"],
    ),
    (
        "notify_intents",
        &["cause_kind", "cause_id", "recipient_key"],
    ),
    ("mail_handoffs", &["handoff_seq"]),
    ("epoch_lines", &["epoch"]),
    ("project_feed", &["epoch", "position"]),
];

#[derive(Debug, PartialEq, Eq)]
struct Table {
    primary: Vec<(String, String, i64)>,
    unique: Vec<Vec<String>>,
    keys: Vec<String>,
    schema: Vec<String>,
    rows: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    names: Vec<String>,
    tables: BTreeMap<String, Table>,
    feed: Vec<(i64, String, Option<String>)>,
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn database(fixture: &BoardFixture) -> FixtureResult<PathBuf> {
    database_for(fixture, &fixture.project)
}

fn database_for(fixture: &BoardFixture, project: &str) -> FixtureResult<PathBuf> {
    if fixture.host.is_some() {
        return Err("readback requires the owning Host to be joined first".into());
    }
    let projects = fixture.directory.join("projects");
    let mut paths = Vec::new();
    let legacy = projects.join(format!("{project}.sqlite"));
    if legacy.is_file() {
        paths.push(legacy);
    }
    if let Ok(entries) = std::fs::read_dir(projects.join(project)) {
        for entry in entries {
            let path = entry?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "sqlite")
            {
                paths.push(path);
            }
        }
    }
    if paths.len() != 1 {
        return Err(format!("expected one owned epoch file, found {paths:?}").into());
    }
    let path = paths.pop().ok_or("epoch file absent")?.canonicalize()?;
    if !path.starts_with(fixture.root.path().canonicalize()?) {
        return Err("epoch file escaped fixture".into());
    }
    Ok(path)
}

async fn strings(
    connection: &mut sqlx_turso::TursoConnection,
    statement: &str,
) -> FixtureResult<Vec<String>> {
    // SQL is fixture-owned: literal table/key allowlist and double-quoted schema identifiers.
    sqlx_turso::sqlx::query::<sqlx_turso::Turso>(AssertSqlSafe(statement))
        .fetch_all(connection)
        .await?
        .iter()
        .map(|row| row.try_get("value").map_err(Into::into))
        .collect()
}

async fn unique_keys(
    connection: &mut sqlx_turso::TursoConnection,
    name: &str,
) -> FixtureResult<Vec<Vec<String>>> {
    let indexes = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
        r#"SELECT name,"unique" FROM pragma_index_list(?) ORDER BY seq"#,
    )
    .bind(name)
    .fetch_all(&mut *connection)
    .await?;
    let mut unique = Vec::new();
    for index in indexes {
        if index.try_get::<i64, _>("unique")? == 1 {
            let index = index.try_get::<String, _>("name")?;
            let columns = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
                "SELECT name FROM pragma_index_info(?) ORDER BY seqno",
            )
            .bind(index)
            .fetch_all(&mut *connection)
            .await?;
            unique.push(
                columns
                    .iter()
                    .map(|column| column.try_get::<String, _>("name"))
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
    }
    unique.sort();
    Ok(unique)
}

async fn read_tables(connection: &mut sqlx_turso::TursoConnection) -> FixtureResult<Snapshot> {
    let names = strings(
        connection,
        "SELECT name AS value FROM sqlite_schema WHERE type='table' ORDER BY name",
    )
    .await?;
    let mut tables = BTreeMap::new();
    for &(name, key) in KEYS {
        let table = quoted(name);
        let columns = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
            r#"SELECT name,type,"notnull",pk FROM pragma_table_info(?) ORDER BY cid"#,
        )
        .bind(name)
        .fetch_all(&mut *connection)
        .await?;
        let mut primary = Vec::new();
        let mut projection = Vec::new();
        for column in &columns {
            let name = column.try_get::<String, _>("name")?;
            projection.push(format!("quote({})", quoted(&name)));
            if column.try_get::<i64, _>("pk")? != 0 {
                primary.push((
                    name,
                    column.try_get::<String, _>("type")?,
                    column.try_get::<i64, _>("notnull")?,
                ));
            }
        }
        let unique = unique_keys(connection, name).await?;
        let schema=strings(connection,&format!("SELECT json_object('type',type,'name',name,'sql',sql) AS value FROM sqlite_schema WHERE tbl_name='{name}' ORDER BY type,name")).await?;
        let keys = strings(
            connection,
            &format!(
                "SELECT {} AS value FROM {table} ORDER BY {}",
                quoted(key),
                quoted(key)
            ),
        )
        .await?;
        let rows = strings(
            connection,
            &format!(
                "SELECT json_array({}) AS value FROM {table} ORDER BY {} LIMIT 10001",
                projection.join(","),
                quoted(key)
            ),
        )
        .await?;
        if rows.len() > 10000 {
            return Err(format!("fixture exceeded bounded readback: {name}").into());
        }
        tables.insert(
            name.to_owned(),
            Table {
                primary,
                unique,
                keys,
                schema,
                rows,
            },
        );
    }
    let feed = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
        "SELECT position,time_basis,router_time FROM project_feed ORDER BY position",
    )
    .fetch_all(&mut *connection)
    .await?
    .iter()
    .map(|row| {
        Ok((
            row.try_get("position")?,
            row.try_get("time_basis")?,
            row.try_get("router_time")?,
        ))
    })
    .collect::<FixtureResult<Vec<_>>>()?;
    Ok(Snapshot {
        names,
        tables,
        feed,
    })
}

async fn snapshot(fixture: &BoardFixture) -> FixtureResult<Snapshot> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(database(fixture)?)
        .create_if_missing(false)
        .read_only(true)
        .connect()
        .await?;
    let result = read_tables(&mut connection).await;
    let close = connection.close().await;
    match (result, close) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
        (Err(read), Err(close)) => Err(format!("readback: {read}; close: {close}").into()),
    }
}

fn assert_schema(snapshot: &Snapshot, fixture: &BoardFixture) -> FixtureResult {
    assert!(
        snapshot
            .names
            .iter()
            .any(|name| name == "capture_feed_marks")
    );
    for retired in [
        "record_identities",
        "fleet_records",
        "feed_checkpoint",
        "writer_epochs",
        "request_ledger",
        "legacy_cursor_mappings",
    ] {
        assert!(
            !snapshot.names.iter().any(|name| name == retired),
            "retired table {retired}"
        );
    }
    assert!(
        !snapshot
            .names
            .iter()
            .any(|name| name.starts_with("_replication_"))
    );
    for &(name, key) in KEYS {
        let table = snapshot.tables.get(name).ok_or("replicated table absent")?;
        assert_eq!(
            table.primary,
            vec![(key.to_owned(), "TEXT".to_owned(), 1)],
            "{name}"
        );
        for key in &table.keys {
            let parsed = uuid::Uuid::parse_str(key)?;
            assert_eq!(parsed.get_version_num(), 7, "{name}: {key}");
            assert_eq!(&parsed.to_string(), key);
        }
        if let Some((_, columns)) = BUSINESS_KEYS.iter().find(|(table, _)| *table == name) {
            assert!(
                table
                    .unique
                    .contains(&columns.iter().map(|column| (*column).to_owned()).collect()),
                "{name} business key"
            );
        }
    }
    for (table, key) in [
        ("project_state", &fixture.project),
        ("workstreams", &fixture.workstream),
    ] {
        assert!(
            snapshot
                .tables
                .get(table)
                .is_some_and(|table| table.keys.contains(key))
        );
    }
    assert!(!snapshot.feed.is_empty());
    for (index, (position, basis, time)) in snapshot.feed.iter().enumerate() {
        assert_eq!(*position, i64::try_from(index)? + 1);
        assert_eq!(basis, "router");
        assert!(time.is_some());
    }
    Ok(())
}

pub async fn stopped_values(
    fixture: &BoardFixture,
    statement: &'static str,
) -> FixtureResult<Vec<String>> {
    stopped_project_values(fixture, &fixture.project, statement).await
}

pub async fn stopped_project_values(
    fixture: &BoardFixture,
    project: &str,
    statement: &'static str,
) -> FixtureResult<Vec<String>> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(database_for(fixture, project)?)
        .create_if_missing(false)
        .read_only(true)
        .connect()
        .await?;
    let result = strings(&mut connection, statement).await;
    let closed = connection.close().await;
    match (result, closed) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
        (Err(read), Err(close)) => Err(format!("read: {read}; close: {close}").into()),
    }
}

pub async fn damage_trail_link(fixture: &BoardFixture, row: &str) -> FixtureResult {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(database(fixture)?)
        .create_if_missing(false)
        .connect()
        .await?;
    let result = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
        "INSERT INTO work_trail_links(link_id,row_id,place,target) VALUES (?,?,0,?)",
    )
    .bind(super::fixture::identity())
    .bind(row)
    .bind(r#"{"kind":"invalid-evidence-variant"}"#)
    .execute(&mut connection)
    .await;
    let closed = connection.close().await;
    result?;
    closed?;
    Ok(())
}

pub async fn damage_thread(fixture: &BoardFixture, project: &str, thread: &str) -> FixtureResult {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(database_for(fixture, project)?)
        .create_if_missing(false)
        .connect()
        .await?;
    let result = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
        "UPDATE threads SET state='invalid-state' WHERE thread_id=?",
    )
    .bind(thread)
    .execute(&mut connection)
    .await;
    let closed = connection.close().await;
    assert_eq!(result?.rows_affected(), 1);
    closed?;
    Ok(())
}

pub async fn assert_reopen(fixture: &mut BoardFixture) -> FixtureResult {
    // Wrong build: startup remints a key, drops a decision, loses feed, or alters a typed row.
    let feed = fixture.feed().await?;
    let before_manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.directory.join("service.json"))?)?;
    fixture.stop_host().await?;
    let before = snapshot(fixture).await?;
    assert_schema(&before, fixture)?;
    fixture.reopen_host().await?;
    let after_manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.directory.join("service.json"))?)?;
    assert_eq!(
        before_manifest.get("serviceId"),
        after_manifest.get("serviceId")
    );
    assert_ne!(
        before_manifest.get("serviceEpoch"),
        after_manifest.get("serviceEpoch")
    );
    assert_eq!(fixture.feed().await?, feed);
    fixture.stop_host().await?;
    assert_eq!(snapshot(fixture).await?, before);
    Ok(())
}
