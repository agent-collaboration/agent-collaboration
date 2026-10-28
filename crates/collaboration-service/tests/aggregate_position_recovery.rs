//! Explicit saved project positions survive an unavailable store and its later return.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
#[path = "support/timed_board_posts.rs"]
mod timed_board_posts;
use board_fixture::{BoardFixture, TestError};
use message_board::*;
use std::collections::BTreeMap;
use timed_board_posts::{TimedPostInput, append_post_at};

async fn head(fixture: &BoardFixture, project: &ProjectId) -> Result<FeedCursor, TestError> {
    let store = fixture
        ._registry
        .existing_project(project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let cursor = read.high_water();
    read.finish().await?;
    Ok(cursor)
}
async fn position(
    fixture: &BoardFixture,
    project: &ProjectId,
    post: &PostId,
) -> Result<FeedCursor, TestError> {
    let store = fixture
        ._registry
        .existing_project(project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let record = read.record(&post.clone().into()).await?.ok_or("post")?;
    read.finish().await?;
    record
        .envelope()
        .position
        .project_cursor()
        .ok_or_else(|| "project position".into())
}
#[derive(Clone, Copy)]
enum ResumeSelection {
    After,
    InclusiveRange,
}

async fn saved_position_recovery(selection: ResumeSelection) -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (healthy, _) = fixture.project().await?;
    let (returning, _) = fixture.project().await?;
    let mut saved = AggregateCursor {
        per_project: BTreeMap::from([
            (healthy.clone(), head(&fixture, &healthy).await?),
            (returning.clone(), head(&fixture, &returning).await?),
        ]),
        sweep: None,
    };
    let first = append_post_at(
        &fixture,
        &healthy,
        TimedPostInput {
            time: "2026-10-12T00:03:20Z",
            thread: None,
        },
    )
    .await?;
    // Range must exhaust A on its first page; a second A post masked R12.
    let last = if matches!(selection, ResumeSelection::After) {
        Some(
            append_post_at(
                &fixture,
                &healthy,
                TimedPostInput {
                    time: "2026-10-12T00:05:00Z",
                    thread: None,
                },
            )
            .await?,
        )
    } else {
        None
    };
    let owed = append_post_at(
        &fixture,
        &returning,
        TimedPostInput {
            time: "2026-10-12T00:02:30Z",
            thread: None,
        },
    )
    .await?;
    let ends = AggregateCursor {
        per_project: BTreeMap::from([
            (healthy.clone(), head(&fixture, &healthy).await?),
            (returning.clone(), head(&fixture, &returning).await?),
        ]),
        sweep: None,
    };
    let request_selection = match selection {
        ResumeSelection::After => PostSelection::AfterPosition {
            after: ReadPosition::Aggregate {
                cursor: saved.clone(),
            },
        },
        ResumeSelection::InclusiveRange => {
            saved
                .per_project
                .insert(healthy.clone(), position(&fixture, &healthy, &first).await?);
            saved.per_project.insert(
                returning.clone(),
                position(&fixture, &returning, &owed).await?,
            );
            PostSelection::Range {
                from: ReadPosition::Aggregate {
                    cursor: saved.clone(),
                },
                to: ReadPosition::Aggregate { cursor: ends },
            }
        }
    };
    // Inclusive `from` is not an exclusive cursor. Without B's snapshot the
    // first page cannot know the predecessor's epoch (PD1, decision1044).
    let expected_returning = match selection {
        ResumeSelection::After => saved.per_project.get(&returning),
        ResumeSelection::InclusiveRange => None,
    };
    let fixture = fixture.restart_read_fixture().await?;
    let path = fixture
        ._directory
        .path()
        .join("projects")
        .join(returning.as_str())
        .join("e1.sqlite");
    let held = fixture._directory.path().join("held-returning.sqlite");
    std::fs::rename(&path, &held)?;
    let client = fixture.api.client("saved position recovery").await?;
    let mut request = PostListRequest {
        scope: PostListScope::Board {
            board_id: fixture.board.clone(),
        },
        selection: request_selection,
        page: PageRequest {
            limit: PageLimit::try_from(1)?,
            cursor: None,
        },
    };
    let page = client.post_list(request.clone()).await?.page;
    assert_eq!(
        page.records
            .iter()
            .map(|post| post.post_id.clone())
            .collect::<Vec<_>>(),
        vec![first.clone()]
    );
    assert_eq!(page.unavailable, vec![returning.clone()]);
    assert_eq!(
        page.next.per_project.get(&returning),
        expected_returning,
        "Range omits unreadable B; AfterPosition preserves its actual saved row"
    );
    assert_eq!(
        page.next.sweep.as_ref().map(|key| key.time),
        Some(MergeTime::Known("2026-10-12T00:03:20Z".parse()?))
    );
    request.page.cursor = Some(page.next_cursor.ok_or("continuation")?);
    std::fs::rename(&held, &path)?;
    let mut seen = vec![first.clone()];
    let second = client.post_list(request.clone()).await?.page;
    assert!(second.unavailable.is_empty());
    assert_eq!(
        second
            .records
            .iter()
            .map(|post| post.post_id.clone())
            .collect::<Vec<_>>(),
        vec![owed.clone()]
    );
    assert_eq!(
        second.records.first().ok_or("owed")?.text.as_str(),
        "2026-10-12T00:02:30Z"
    );
    seen.extend(second.records.into_iter().map(|post| post.post_id));
    if let Some(last) = last {
        request.page.cursor = Some(second.next_cursor.ok_or("last continuation")?);
        let third = client.post_list(request).await?.page;
        seen.extend(third.records.into_iter().map(|post| post.post_id));
        assert!(third.next_cursor.is_none());
        assert_eq!(seen, vec![first, owed, last]);
    } else {
        assert!(second.next_cursor.is_none());
        assert_eq!(seen, vec![first, owed]);
    }
    fixture.stop().await
}
#[tokio::test]
async fn after_position_retains_unavailable_b_behind_the_sweep() -> Result<(), TestError> {
    Box::pin(saved_position_recovery(ResumeSelection::After)).await
}
#[tokio::test]
async fn inclusive_range_retains_the_unavailable_boundary_record() -> Result<(), TestError> {
    Box::pin(saved_position_recovery(ResumeSelection::InclusiveRange)).await
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions retain the observed contract; Result propagates fixture failures."
)]
async fn an_unavailable_only_page_terminates_without_losing_saved_positions()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (first, _) = fixture.project().await?;
    let (second, _) = fixture.project().await?;
    let saved = AggregateCursor {
        per_project: BTreeMap::from([
            (first.clone(), head(&fixture, &first).await?),
            (second.clone(), head(&fixture, &second).await?),
        ]),
        sweep: Some(MergeKey {
            time: MergeTime::Known("2026-10-12T00:03:20Z".parse()?),
            project_id: first.clone(),
            position: FeedPosition::try_from(1)?,
        }),
    };
    let fixture = fixture.restart_read_fixture().await?;
    let mut paths = Vec::new();
    for project in [&first, &second] {
        let path = fixture
            ._directory
            .path()
            .join("projects")
            .join(project.as_str())
            .join("e1.sqlite");
        let held = fixture
            ._directory
            .path()
            .join(format!("held-{}.sqlite", project.as_str()));
        std::fs::rename(&path, &held)?;
        paths.push((held, path));
    }
    let client = fixture.api.client("unavailable-only positions").await?;
    let page = client
        .post_list(PostListRequest {
            scope: PostListScope::Board {
                board_id: fixture.board.clone(),
            },
            selection: PostSelection::AfterPosition {
                after: ReadPosition::Aggregate {
                    cursor: saved.clone(),
                },
            },
            page: PageRequest {
                limit: PageLimit::try_from(1)?,
                cursor: None,
            },
        })
        .await?
        .page;
    assert!(page.records.is_empty());
    assert!(page.next_cursor.is_none());
    assert_eq!(
        page.unavailable
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        [first, second].into_iter().collect()
    );
    assert_eq!(page.next, saved);
    for (held, path) in paths {
        std::fs::rename(held, path)?;
    }
    fixture.stop().await
}

async fn exhausted_range_retries(readable_posts: usize) -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (healthy, _) = fixture.project().await?;
    let (returning, _) = fixture.project().await?;
    let mut first_healthy = head(&fixture, &healthy).await?;
    let mut readable = Vec::new();
    for time in ["2026-10-12T00:03:20Z", "2026-10-12T00:05:00Z"]
        .into_iter()
        .take(readable_posts)
    {
        let post =
            append_post_at(&fixture, &healthy, TimedPostInput { time, thread: None }).await?;
        if readable.is_empty() {
            first_healthy = position(&fixture, &healthy, &post).await?;
        }
        readable.push(post);
    }
    let owed = append_post_at(
        &fixture,
        &returning,
        TimedPostInput {
            time: "2026-10-12T00:02:30Z",
            thread: None,
        },
    )
    .await?;
    let lower = AggregateCursor {
        per_project: BTreeMap::from([
            (healthy.clone(), first_healthy),
            (
                returning.clone(),
                position(&fixture, &returning, &owed).await?,
            ),
        ]),
        sweep: None,
    };
    let upper = AggregateCursor {
        per_project: BTreeMap::from([
            (healthy.clone(), head(&fixture, &healthy).await?),
            (returning.clone(), head(&fixture, &returning).await?),
        ]),
        sweep: None,
    };
    let fixture = fixture.restart_read_fixture().await?;
    let project_path = |project: &ProjectId| {
        fixture
            ._directory
            .path()
            .join("projects")
            .join(project.as_str())
            .join("e1.sqlite")
    };
    let returning_path = project_path(&returning);
    let held_returning = fixture
        ._directory
        .path()
        .join("held-range-returning.sqlite");
    std::fs::rename(&returning_path, &held_returning)?;
    let client = fixture.api.client("exhausted inclusive Range").await?;
    let mut request = PostListRequest {
        scope: PostListScope::Board {
            board_id: fixture.board.clone(),
        },
        selection: PostSelection::Range {
            from: ReadPosition::Aggregate { cursor: lower },
            to: ReadPosition::Aggregate { cursor: upper },
        },
        page: PageRequest {
            limit: PageLimit::try_from(1)?,
            cursor: None,
        },
    };
    for expected in readable {
        let page = client.post_list(request.clone()).await?.page;
        assert_eq!(
            page.records
                .iter()
                .map(|post| post.post_id.clone())
                .collect::<Vec<_>>(),
            vec![expected]
        );
        assert!(page.unavailable.contains(&returning));
        assert!(!page.next.per_project.contains_key(&returning));
        request.page.cursor = Some(
            page.next_cursor
                .ok_or("readable exhaustion dropped the Range retry")?,
        );
    }
    // Repeat an empty continuation while the in-scope debt is still unreadable.
    for _ in 0..2 {
        let page = client.post_list(request.clone()).await?.page;
        assert!(page.records.is_empty());
        assert!(page.unavailable.contains(&returning));
        assert!(!page.next.per_project.contains_key(&returning));
        request.page.cursor = Some(
            page.next_cursor
                .ok_or("empty Range retry lost its inclusive lower bound")?,
        );
    }
    std::fs::rename(&held_returning, &returning_path)?;
    let page = client.post_list(request).await?.page;
    assert_eq!(
        page.records
            .iter()
            .map(|post| post.post_id.clone())
            .collect::<Vec<_>>(),
        vec![owed]
    );
    assert!(page.unavailable.is_empty());
    assert!(
        page.next_cursor.is_none(),
        "all originally scoped Range debt is exhausted"
    );
    fixture.stop().await
}

#[tokio::test]
async fn an_initially_unavailable_only_range_retains_its_signed_retry() -> Result<(), TestError> {
    exhausted_range_retries(0).await
}

#[tokio::test]
async fn a_later_exhausted_range_page_retains_its_signed_retry() -> Result<(), TestError> {
    exhausted_range_retries(2).await
}
