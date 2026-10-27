//! Native metadata keeps unknown import time and applies a precise cross-project sweep cut.

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
mod project_storage_support;
use message_board::*;
use message_board_storage::{
    FeedPageBounds, FeedPageDirection, ImportRowKeys, ImportRowKind, PostPageFilter, PostPageScope,
    ProjectWriteTransaction,
};
use project_storage_support::*;
use std::sync::Arc;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

struct ImportFixtureKeys;
impl ImportRowKeys for ImportFixtureKeys {
    fn row_key(&self, kind: ImportRowKind, stable_parts: &[&str]) -> String {
        match kind {
            ImportRowKind::Actor => ACTOR_KEY.to_owned(),
            ImportRowKind::EpochLine => LINE_KEY.to_owned(),
            ImportRowKind::FeedEntry => match stable_parts.last() {
                Some(&"1") => "0192a1b2-c3d4-7000-8000-000000000011",
                Some(&"2") => "0192a1b2-c3d4-7000-8000-000000000012",
                Some(&"3") => "0192a1b2-c3d4-7000-8000-000000000013",
                Some(&"4") => "0192a1b2-c3d4-7000-8000-000000000014",
                Some(&"5") => "0192a1b2-c3d4-7000-8000-000000000015",
                _ => "missing feed fixture key",
            }
            .to_owned(),
            _ => "unexpected fixture key kind".to_owned(),
        }
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn imported_known_time_and_unknown_time_round_trip_and_exact_second_sweeps_keep_ties()
-> TestResult {
    // Arrange imported records with both actual stored time and explicit missing time.
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let source = origin(2, "import provenance");
    let (database, store) =
        open_project(&directory.path().join("project.sqlite"), &project, peer()).await;
    let base = project_record(&project, &source);
    let board = base.envelope().scope.board.clone().ok_or("board fixture")?;
    let envelope = |id, time| RecordEnvelope {
        id,
        time,
        ..base.envelope().clone()
    };
    let root = PostId::generate();
    let thread = ThreadId::from(root.clone());
    let first_known = PostId::generate();
    let second_known = PostId::generate();
    let known_time = base.envelope().time;
    let mut import = ProjectWriteTransaction::for_import(
        project.clone(),
        database.begin_write().await?,
        fleet_admission::ImportProvenance {
            installation: source.installation.clone(),
            machine: source.machine.clone(),
        },
        Arc::new(ImportFixtureKeys),
    )
    .await?;
    import.append_record(base.clone()).await?;
    import
        .append_record(Record::Thread(Thread {
            envelope: envelope(thread.clone().into(), known_time),
            thread_id: thread.clone(),
            about: ThreadSubject::Project {
                line: SubjectLine::try_from("imported thread".to_owned())?,
            },
            imported: true,
            state: ThreadState::Open,
            orchestrator: None,
            implementer: None,
        }))
        .await?;
    let mut cursors = Vec::new();
    for (id, time) in [
        (root.clone(), RecordTime::LegacyUnknown),
        (first_known.clone(), known_time),
        (second_known.clone(), known_time),
    ] {
        let placement = if id == root {
            Placement::Project {
                project_id: project.clone(),
            }
        } else {
            Placement::Thread {
                thread_id: thread.clone(),
            }
        };
        cursors.push(
            import
                .append_record(Record::Post(Post {
                    envelope: envelope(id.clone().into(), time),
                    post_id: id,
                    board_id: board.clone(),
                    project_id: project.clone(),
                    placement,
                    actor: reader(1),
                    acting_for: None,
                    text: PostText::try_from("imported post".to_owned())?,
                    references: PostReferences::try_from(Vec::new())?,
                    position: base.envelope().position.clone(),
                    posted_as_role: None,
                }))
                .await?,
        );
    }
    import.commit().await?;

    // Act: page native metadata and decode immutable rows using the same sweep format as APIs.
    let mut read = store.snapshot().await?;
    let mut bounds = FeedPageBounds {
        after: FeedPosition::ZERO,
        through: read.high_water().position,
        direction: FeedPageDirection::Forward,
        limit: 20,
        sweep: None,
    };
    let filter = || PostPageFilter {
        scope: PostPageScope::All,
        search: None,
    };
    let all = read.post_page_keys(&bounds, filter()).await?;
    let unknown = cursors.first().ok_or("unknown cursor")?;
    let first = cursors.get(1).ok_or("first known cursor")?;
    bounds.sweep = Some(MergeKey {
        time: MergeTime::Unknown,
        project_id: project.clone(),
        position: unknown.position,
    });
    let after_unknown = read.post_page_keys(&bounds, filter()).await?;
    bounds.sweep = Some(MergeKey {
        time: known_time.into(),
        project_id: project.clone(),
        position: first.position,
    });
    let after_known = read.post_page_keys(&bounds, filter()).await?;
    let entries = read
        .feed_entry_batch(&all)
        .await?
        .into_values()
        .map(|entry| entry.decode(false))
        .collect::<Result<Vec<_>, _>>()?;

    // Assert: no invented time and no loss of a same-second, later-position tie.
    assert_eq!(
        all.iter().map(|key| key.time).collect::<Vec<_>>(),
        vec![RecordTime::LegacyUnknown, known_time, known_time]
    );
    assert_eq!(
        after_unknown
            .iter()
            .map(|key| &key.reference)
            .collect::<Vec<_>>(),
        vec![&first_known.clone().into(), &second_known.clone().into()]
    );
    assert_eq!(
        after_known
            .iter()
            .map(|key| &key.reference)
            .collect::<Vec<_>>(),
        vec![&second_known.into()]
    );
    assert_eq!(
        entries.iter().map(|entry| entry.time).collect::<Vec<_>>(),
        vec![RecordTime::LegacyUnknown, known_time, known_time]
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.origin == source && entry.actor == base.envelope().actor)
    );
    read.finish().await?;
    store.close().await;
    Ok(())
}
