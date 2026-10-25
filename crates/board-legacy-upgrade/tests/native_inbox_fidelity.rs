#![allow(clippy::panic_in_result_fn)]
mod support;
use message_board::*;
use message_board_storage::{FeedPageBounds, FeedPageDirection};
use std::collections::BTreeSet;
use support::*;

#[tokio::test]
async fn mixed_main_and_reply_bookmarks_keep_the_shipped_post_selection() -> TestResult {
    assert_selection("legacy-complete.sqlite", 21, true).await
}

#[tokio::test]
async fn topic_watch_does_not_invent_legacy_thread_reply_coverage() -> TestResult {
    assert_selection("watch-scope.sqlite", 6, false).await
}

#[tokio::test]
async fn topic_and_explicit_thread_watch_keep_watched_and_unwatched_reply_sets() -> TestResult {
    assert_selection("mixed-watch-scope.sqlite", 8, true).await
}

async fn assert_selection(
    name: &str,
    expected_comparisons: usize,
    require_unread: bool,
) -> TestResult {
    let (_directory, path) = fixture_copy(name)?;
    let prepared = prepare_path(&path).await?;
    let source =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut identities=source.prepare("SELECT identity_key,kind,service_id,endpoint_id,session_id,human_id FROM board_identities ORDER BY identity_key")?;
    let readers = identities
        .query_map([], |row| {
            Ok((
                row.get::<_, String>("identity_key")?,
                row.get::<_, String>("kind")?,
                row.get::<_, Option<String>>("service_id")?,
                row.get::<_, Option<String>>("endpoint_id")?,
                row.get::<_, Option<String>>("session_id")?,
                row.get::<_, Option<String>>("human_id")?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut comparisons = 0;
    let mut nonempty = 0;
    for (key, kind, service, endpoint, session, human) in readers {
        let reader = match (kind.as_str(), service, endpoint, session, human) {
            ("human", None, None, None, Some(human)) => Identity::Human {
                human_id: HumanId::try_from(human)?,
            },
            ("session", Some(service), Some(endpoint), Some(session), None) => Identity::Session {
                session: SessionRef {
                    endpoint: SessionEndpointRef {
                        service_id: ServiceId::try_from(service)?,
                        endpoint_id: EndpointId::try_from(endpoint)?,
                    },
                    session_id: SessionId::try_from(session)?,
                },
            },
            _ => return Err("invalid test source identity".into()),
        };
        for project in &prepared.projects {
            // This is the shipped ffced1c1 inbox predicate, independent of the
            // importer's position mapping. Topic bookmarks cover only main posts.
            let mut expected_query=source.prepare("SELECT a.message_id FROM board_activity a LEFT JOIN project_reader_state prs ON prs.reader_key=?1 AND prs.project_id=a.project_id LEFT JOIN topic_read_bookmarks tb ON tb.reader_key=?1 AND tb.topic_id=a.topic_id LEFT JOIN thread_watches w ON w.reader_key=?1 AND w.root_id=a.root_id LEFT JOIN thread_read_bookmarks rb ON rb.reader_key=?1 AND rb.root_id=a.root_id WHERE a.topic_id=?2 AND a.actor_key<>?1 AND ((a.kind='mainMessageCreated' AND prs.main_start IS NOT NULL AND a.activity_sequence>prs.main_start AND a.activity_sequence>COALESCE(tb.through_activity,0)) OR (a.kind='threadMessageCreated' AND w.active=1 AND a.activity_sequence>w.starts_after_activity AND a.activity_sequence>COALESCE(rb.through_activity,0))) ORDER BY a.activity_sequence")?;
            let expected = expected_query
                .query_map(
                    rusqlite::params![key, project.store.project_id().as_str()],
                    |row| row.get::<_, String>("message_id"),
                )?
                .collect::<Result<BTreeSet<_>, _>>()?;
            let mut snapshot = project.store.snapshot().await?;
            let bounds = FeedPageBounds {
                after: FeedPosition::ZERO,
                through: snapshot.high_water().position,
                direction: FeedPageDirection::Forward,
                limit: 1000,
                sweep: None,
            };
            let actual = snapshot
                .inbox_page_keys(&bounds, &reader, InboxReadMode::Unread, true)
                .await?
                .into_iter()
                .filter(|row| row.reference.kind == RecordKind::Post)
                .map(|row| row.reference.id.as_str().to_owned())
                .collect::<BTreeSet<_>>();
            assert_eq!(
                actual,
                expected,
                "reader {key}, project {}",
                project.store.project_id().as_str()
            );
            comparisons += 1;
            if !expected.is_empty() {
                nonempty += 1;
            }
            snapshot.finish().await?;
        }
    }
    assert_eq!(comparisons, expected_comparisons);
    if require_unread {
        assert!(nonempty > 0, "the source must exercise an unread post set");
    }
    prepared.close().await;
    Ok(())
}
