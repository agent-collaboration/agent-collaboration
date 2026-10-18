//! Explicit saved project positions survive an unavailable store and its later return.
#[path = "support/board_fixture.rs"]
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
    match record.envelope().position {
        RecordPosition::Project(cursor) => Ok(cursor),
        _ => Err("project position".into()),
    }
}
#[derive(Clone, Copy)]
enum ResumeSelection {
    After,
    InclusiveRange,
}

#[allow(clippy::panic_in_result_fn)]
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
    let last = append_post_at(
        &fixture,
        &healthy,
        TimedPostInput {
            time: "2026-10-12T00:05:00Z",
            thread: None,
        },
    )
    .await?;
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
    let mut expected_returning = *saved.per_project.get(&returning).ok_or("saved B")?;
    if matches!(selection, ResumeSelection::InclusiveRange) {
        expected_returning.position =
            FeedPosition::try_from(expected_returning.position.get() - 1)?;
    }
    let fixture = fixture.restart_read_fixture().await?;
    let path = fixture
        ._directory
        .path()
        .join("projects")
        .join(format!("{}.sqlite", returning.as_str()));
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
        Some(&expected_returning),
        "unavailable B lost its effective saved position"
    );
    assert_eq!(
        page.next.sweep.as_ref().map(|key| key.router_time),
        Some("2026-10-12T00:03:20Z".parse()?)
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
    request.page.cursor = Some(second.next_cursor.ok_or("last continuation")?);
    let third = client.post_list(request).await?.page;
    seen.extend(third.records.into_iter().map(|post| post.post_id));
    assert!(third.next_cursor.is_none());
    assert_eq!(seen, vec![first, owed, last]);
    fixture.stop().await
}
#[tokio::test]
async fn after_position_retains_unavailable_b_behind_the_sweep() -> Result<(), TestError> {
    saved_position_recovery(ResumeSelection::After).await
}
#[tokio::test]
async fn inclusive_range_retains_the_unavailable_boundary_record() -> Result<(), TestError> {
    saved_position_recovery(ResumeSelection::InclusiveRange).await
}
#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
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
            router_time: "2026-10-12T00:03:20Z".parse()?,
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
            .join(format!("{}.sqlite", project.as_str()));
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
