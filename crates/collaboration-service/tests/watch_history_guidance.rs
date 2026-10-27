#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use serde_json::json;
use sqlx::{ConnectOptions, Connection};

async fn watch(
    fixture: &BoardFixture,
    reader: &Identity,
    thread: &ThreadId,
    tool: &str,
) -> Result<serde_json::Value, TestError> {
    let mut request = fixture.header();
    request.as_object_mut().ok_or("watch request")?.extend(
        json!({"actor":reader,"thread":{"thread":thread}})
            .as_object()
            .ok_or("watch fields")?
            .clone(),
    );
    Ok(fixture.api.call(tool, request).await?)
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn history_guidance_exists_only_for_queryable_scoped_posts() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("history guidance proof").await?;
    let (_, _, empty) = create_location(&fixture, "empty").await?;
    let (_, _, active) = create_location(&fixture, "active").await?;
    post_at_placement(
        &fixture,
        Placement::Project { project_id: active },
        json!({"actor":{"kind":"human","humanId":"other-author"},"text":"other project"}),
    )
    .await?;
    for (name, expect_history) in [("empty-reader", false), ("history-reader", true)] {
        if expect_history {
            post_at_placement(
                &fixture,
                Placement::Project {
                    project_id: empty.clone(),
                },
                json!({"actor":{"kind":"human","humanId":"root-author"},"text":"root"}),
            )
            .await?;
        }
        let initialized = client
            .inbox_fetch(initialized_request(empty.clone(), human_reader(name)?)?)
            .await?;
        let ProjectWriteCallResult::Replied { reply } = &initialized
            .page
            .initializations
            .first()
            .ok_or("initialization")?
            .outcome
        else {
            return Err("initialization unanswered".into());
        };
        let through = reply.reader.as_ref().ok_or("reader boundary")?.read_through;
        let queried = client
            .post_list(PostListRequest {
                scope: PostListScope::Project {
                    project_id: empty.clone(),
                },
                selection: PostSelection::Range {
                    from: ReadPosition::Project {
                        cursor: FeedCursor {
                            epoch: through.epoch,
                            position: FeedPosition::ZERO,
                        },
                    },
                    to: ReadPosition::Project { cursor: through },
                },
                page: PageRequest::default(),
            })
            .await?;
        assert_eq!(
            !queried.page.records.is_empty(),
            expect_history,
            "initialization boundary returns only actual scoped history"
        );
    }
    let root = client
        .post_list(PostListRequest {
            scope: PostListScope::Project {
                project_id: empty.clone(),
            },
            selection: PostSelection::Latest,
            page: PageRequest::default(),
        })
        .await?
        .page
        .records
        .first()
        .ok_or("root")?
        .post_id
        .clone();
    let thread = ThreadId::from(root);
    let empty_reader = human_reader("empty-watcher")?;
    accepted(&watch(&fixture, &empty_reader, &thread, "thread_watch").await?);
    let empty_watch = client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (thread.clone()).into(),
            reader: ReadActor::Identified {
                reader: empty_reader,
            },
        })
        .await?
        .watch_status
        .ok_or("watch")?;
    assert!(empty_watch.earlier_unwatched_range.is_none());
    assert!(!empty_watch.message.contains("earlierUnwatchedRange"));
    let reply = post_at_placement(
        &fixture,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"reply-author"},"text":"reply"}),
    )
    .await?;
    let reader = human_reader("history-watcher")?;
    accepted(&watch(&fixture, &reader, &thread, "thread_watch").await?);
    let history = client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (thread.clone()).into(),
            reader: ReadActor::Identified { reader },
        })
        .await?
        .watch_status
        .ok_or("watch")?;
    let range = history.earlier_unwatched_range.ok_or("history range")?;
    assert_eq!(
        range.scope,
        PostListScope::Thread {
            thread: ThreadRef {
                thread,
                project: Some(empty.clone())
            }
        }
    );
    let actual = client
        .post_show(PostShowRequest {
            post_id: reply.clone(),
        })
        .await?
        .0;
    assert!(
        matches!(range.selection,PostSelection::Range {to:ReadPosition::Project {cursor},..} if actual.position.project_cursor() == Some(cursor))
    );
    assert!(history.message.contains("earlierUnwatchedRange"));
    let queried = client
        .post_list(PostListRequest {
            scope: range.scope,
            selection: range.selection,
            page: PageRequest::default(),
        })
        .await?;
    assert!(
        queried
            .page
            .records
            .iter()
            .any(|post| post.post_id == reply)
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn inactive_watch_hides_subscription_start_without_rewriting_history() -> Result<(), TestError>
{
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("inactive watch proof").await?;
    let (_, _, project) = create_location(&fixture, "watch").await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"author"},"text":"root"}),
    )
    .await?;
    let thread = ThreadId::from(root);
    let reader = human_reader("watcher")?;
    accepted(&watch(&fixture, &reader, &thread, "thread_watch").await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project store")?;
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let reader_key = serde_json::to_string(&RecordedActor::declared(reader.clone()))?;
    let before: i64 = sqlx::query_scalar(
        "SELECT w.start_position FROM thread_watches w JOIN actors a ON a.actor_id=w.reader_actor WHERE a.canonical_identity=? AND w.thread_id=?",
    )
    .bind(&reader_key)
    .bind(thread.as_str())
    .fetch_one(&mut raw)
    .await?;
    let reply = post_at_placement(
        &fixture,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"reply-author"},"text":"reply"}),
    )
    .await?;
    accepted(&watch(&fixture, &reader, &thread, "thread_unwatch").await?);
    let inactive = client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (thread.clone()).into(),
            reader: ReadActor::Identified {
                reader: reader.clone(),
            },
        })
        .await?
        .watch_status
        .ok_or("watch")?;
    assert!(!inactive.watching);
    assert!(inactive.starts_after.is_none());
    let range = inactive
        .earlier_unwatched_range
        .ok_or("unwatched history")?;
    let actual = client
        .post_show(PostShowRequest { post_id: reply })
        .await?
        .0;
    assert!(
        matches!(range.selection,PostSelection::Range {to:ReadPosition::Project {cursor},..} if actual.position.project_cursor() == Some(cursor))
    );
    let stored: i64 = sqlx::query_scalar(
        "SELECT w.start_position FROM thread_watches w JOIN actors a ON a.actor_id=w.reader_actor WHERE a.canonical_identity=? AND w.thread_id=?",
    )
    .bind(reader_key)
    .bind(thread.as_str())
    .fetch_one(&mut raw)
    .await?;
    assert_eq!(stored, before);
    raw.close().await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Retained archive/reader-state claim; Result propagates setup errors.
async fn watch_messages_explain_history_guidance_and_board_lifecycle() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("archive watch proof").await?;
    let (_, board, project) = create_location(&fixture, "lifecycle").await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"author"},"text":"root"}),
    )
    .await?;
    let thread = ThreadId::from(root);
    let reader = human_reader("watcher")?;
    let unwatched = client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (thread.clone()).into(),
            reader: ReadActor::Identified {
                reader: reader.clone(),
            },
        })
        .await?
        .watch_status
        .ok_or("watch")?;
    for word in ["not watched", "watch", "active"] {
        assert!(unwatched.message.contains(word));
    }
    accepted(&watch(&fixture, &reader, &thread, "thread_watch").await?);
    let watched = client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (thread.clone()).into(),
            reader: ReadActor::Identified {
                reader: reader.clone(),
            },
        })
        .await?
        .watch_status
        .ok_or("watch")?;
    assert!(watched.earlier_unwatched_range.is_none());
    assert!(!watched.message.contains("earlierUnwatchedRange"));
    assert!(watched.message.contains("active"));
    let mut archive = fixture.header();
    archive["boardId"] = json!(board);
    accepted(&fixture.api.call("board_archive", archive).await?);
    accepted(&watch(&fixture, &reader, &thread, "thread_unwatch").await?);
    let archived = client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: ThreadRef {
                thread,
                project: Some(project.clone()),
            },
            reader: ReadActor::Identified { reader },
        })
        .await?
        .watch_status
        .ok_or("watch")?;
    assert!(!archived.watching);
    assert!(archived.message.contains("read-only"));
    fixture.stop().await
}
