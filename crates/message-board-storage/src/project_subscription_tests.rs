//! Relational thread scope selects a needed post even when its placement body is corrupt.
use crate::{FeedPageBounds, FeedPageDirection, ProjectStore};
use message_board::*;
use sqlx::{ConnectOptions, Connection};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn post_scope_cannot_hide_corrupt_placement_and_ignores_a_foreign_thread_body()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let path = directory.path().join("post-scope.sqlite");
    let base = crate::project_store_tests::project_record(project.clone());
    let actor = Identity::Human {
        human_id: "owner".to_owned().try_into()?,
    };
    let selected = ThreadId::from(PostId::generate());
    let foreign = ThreadId::from(PostId::generate());
    let selected_post = PostId::generate();
    let foreign_post = PostId::generate();
    let store = ProjectStore::open(&path, project.clone()).await?;
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base.clone()).await?;
    for (thread, post) in [(&selected, &selected_post), (&foreign, &foreign_post)] {
        write
            .append_record(Record::Thread(Thread {
                envelope: RecordEnvelope {
                    id: thread.clone().into(),
                    ..base.envelope().clone()
                },
                thread_id: thread.clone(),
                about: ThreadSubject::Project {
                    line: SubjectLine::try_from("Fixture discussion".to_owned())?,
                },
                imported: false,
                state: ThreadState::Open,
                orchestrator: None,
                implementer: None,
            }))
            .await?;
        write
            .append_record(Record::Post(Post {
                envelope: RecordEnvelope {
                    id: post.clone().into(),
                    actor: RecordedActor::declared(actor.clone()),
                    ..base.envelope().clone()
                },
                post_id: post.clone(),
                board_id: base.envelope().scope.board.clone().ok_or("board")?,
                project_id: project.clone(),
                placement: Placement::Thread {
                    thread_id: thread.clone(),
                },
                actor: actor.clone(),
                acting_for: None,
                posted_as_role: None,
                text: "selected post body".to_owned().try_into()?,
                references: Vec::new().try_into()?,
                position: base.envelope().position.clone(),
            }))
            .await?;
    }
    write.commit().await?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    for (post, must_fail) in [(&foreign_post, false), (&selected_post, true)] {
        let changed = sqlx::query("UPDATE project_feed SET content = json_set(content, '$.record.record.placement.threadId', '') WHERE record_kind = 'post' AND record_id = ?")
            .bind(post.as_str()).execute(&mut observer).await?;
        assert_eq!(changed.rows_affected(), 1);
        let mut read = store.snapshot().await?;
        let bounds = FeedPageBounds {
            after: FeedPosition::ZERO,
            through: read.high_water().position,
            direction: FeedPageDirection::Forward,
            limit: 101,
            sweep: None,
        };
        let keys = read.post_activity_keys(&bounds, Some(&selected)).await;
        if must_fail {
            assert!(
                keys.is_err(),
                "malformed placement of a related post disappeared from its scope"
            );
        } else {
            let keys = keys?;
            assert_eq!(keys.len(), 1);
            let key = keys.into_iter().next().ok_or("selected key")?;
            assert_eq!(key.feed.reference, selected_post.clone().into());
            let bodies = read.feed_entry_batch(&[key.feed]).await?;
            assert_eq!(bodies.len(), 1);
            let entry = bodies.into_values().next().ok_or("body")?.decode(false)?;
            assert!(
                matches!(entry.content, FeedContent::Shown(record) if matches!(record.as_ref(), Record::Post(row) if row.post_id == selected_post))
            );
        }
        read.finish().await?;
    }
    observer.close().await?;
    store.close().await;
    Ok(())
}
