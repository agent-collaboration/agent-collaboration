#![expect(
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
#![allow(clippy::panic_in_result_fn)]
mod support;
use fleet_sync_protocol::SqlValue;
use message_board::*;
use std::collections::{BTreeMap, BTreeSet};
use support::*;

// Reader identity, optional thread scope, translated epoch/position, active choice.
type WatchRow = (String, Option<String>, u64, u64, bool);

fn reader_key(actor: RecordedActor) -> TestResult<String> {
    Ok(
        match actor.declared_identity().ok_or("undeclared watch reader")? {
            Identity::Human { human_id } => format!("human:{}", human_id.as_str()),
            Identity::Session { session } => format!(
                "session:{}:{}:{}",
                session.endpoint.service_id.as_str(),
                session.endpoint.endpoint_id.as_str(),
                session.session_id.as_str(),
            ),
        },
    )
}

#[tokio::test]
async fn native_watch_rows_keep_exact_scopes_choices_and_starts_without_synthetic_opt_outs()
-> TestResult {
    for name in [
        "legacy-complete.sqlite",
        "watch-scope.sqlite",
        "mixed-watch-scope.sqlite",
    ] {
        let (_directory, path) = fixture_copy(name)?;
        let prepared = prepare_path(&path).await?;
        let source = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        for project in &prepared.projects {
            let report = prepared
                .report
                .projects
                .iter()
                .find(|report| &report.project == project.store.project_id())
                .ok_or("project report missing")?;
            let mut expected = BTreeSet::new();
            for sql in [
                "SELECT reader_key,NULL AS thread,starts_after_activity,active FROM topic_watches WHERE topic_id=?",
                "SELECT w.reader_key,w.root_id AS thread,w.starts_after_activity,w.active FROM thread_watches w JOIN board_messages m ON m.message_id=w.root_id WHERE m.topic_id=?",
            ] {
                let mut statement = source.prepare(sql)?;
                let mut rows = statement.query([project.store.project_id().as_str()])?;
                while let Some(row) = rows.next()? {
                    let old: u64 = row.get("starts_after_activity")?;
                    let position = report
                        .old_to_new
                        .range(..=old)
                        .next_back()
                        .map_or(0, |(_, cursor)| cursor.position.get());
                    expected.insert((
                        row.get::<_, String>("reader_key")?,
                        row.get::<_, Option<String>>("thread")?,
                        1,
                        position,
                        row.get::<_, bool>("active")?,
                    ));
                }
            }

            // Read the actual application rows through the owning writer's pinned
            // native snapshot, including inactive rows hidden by inbox APIs.
            let mut snapshot = project.database.snapshot().await?;
            let mut tables: BTreeMap<String, Vec<Vec<SqlValue>>> = BTreeMap::new();
            let mut cursor = None;
            loop {
                let page = snapshot.page(cursor).await?;
                if matches!(
                    page.table.as_str(),
                    "actors" | "project_watches" | "thread_watches"
                ) {
                    tables.entry(page.table).or_default().extend(page.rows);
                }
                cursor = page.next;
                if cursor.is_none() {
                    break;
                }
            }
            snapshot.close().await?;
            let mut actors = BTreeMap::new();
            for row in tables.remove("actors").unwrap_or_default() {
                let [SqlValue::Text(id), SqlValue::Text(canonical)] = row.as_slice() else {
                    return Err("invalid native actor row".into());
                };
                RecordId::try_from(id.clone())?;
                actors.insert(id.clone(), reader_key(serde_json::from_str(canonical)?)?);
            }
            let mut actual: BTreeSet<WatchRow> = BTreeSet::new();
            for table in ["project_watches", "thread_watches"] {
                for row in tables.remove(table).unwrap_or_default() {
                    let (id, actor, thread, epoch, position, active) = match row.as_slice() {
                        [
                            SqlValue::Text(id),
                            SqlValue::Text(actor),
                            SqlValue::Integer(epoch),
                            SqlValue::Integer(position),
                            SqlValue::Integer(active),
                        ] if table == "project_watches" => {
                            (id, actor, None, epoch, position, active)
                        }
                        [
                            SqlValue::Text(id),
                            SqlValue::Text(actor),
                            SqlValue::Text(thread),
                            SqlValue::Integer(epoch),
                            SqlValue::Integer(position),
                            SqlValue::Integer(active),
                        ] if table == "thread_watches" => {
                            ThreadId::try_from(thread.clone())?;
                            (id, actor, Some(thread.clone()), epoch, position, active)
                        }
                        _ => return Err("invalid native watch row".into()),
                    };
                    RecordId::try_from(id.clone())?;
                    let active = match active {
                        0 => false,
                        1 => true,
                        _ => return Err("invalid watch choice".into()),
                    };
                    let reader = actors
                        .get(actor)
                        .ok_or("native watch actor missing")?
                        .clone();
                    assert!(
                        actual.insert((
                            reader,
                            thread,
                            u64::try_from(*epoch)?,
                            u64::try_from(*position)?,
                            active
                        )),
                        "duplicate native watch"
                    );
                }
            }
            assert_eq!(
                actual,
                expected,
                "{name}, project {}",
                project.store.project_id().as_str()
            );
        }
        prepared.close().await;
    }
    Ok(())
}
