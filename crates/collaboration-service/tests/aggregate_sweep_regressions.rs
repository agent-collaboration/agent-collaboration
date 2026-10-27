//! A merge sweep is monotone even when one project's clock runs backwards.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod board_read_setup;
#[path = "support/timed_board_posts.rs"]
mod timed_board_posts;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::{human_reader, inbox_post_ids, inbox_request, initialized_request};
use message_board::*;
use serde_json::json;
use timed_board_posts::{TimedPostInput, append_post_at};

async fn clock_regression_case(mode: InboxReadMode, page_size: u32) -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("monotone sweep").await?;
    let (project, _) = fixture.project().await?;
    let (foreign, _) = fixture.project().await?;
    let reader = human_reader("clock-sweep-reader")?;
    client
        .inbox_fetch(initialized_request(project.clone(), reader.clone())?)
        .await?;
    let root = append_post_at(
        &fixture,
        &foreign,
        TimedPostInput {
            time: "2026-10-12T00:00:00Z",
            thread: None,
        },
    )
    .await?;
    let thread = ThreadId::from(root);
    let times = if mode == InboxReadMode::Latest {
        [
            "2026-10-12T09:00:00Z",
            "2026-10-12T11:00:00Z",
            "2026-10-12T10:00:00Z",
        ]
    } else {
        [
            "2026-10-12T11:00:00Z",
            "2026-10-12T09:00:00Z",
            "2026-10-12T10:00:00Z",
        ]
    };
    let mut expected = Vec::new();
    for time in times {
        expected
            .push(append_post_at(&fixture, &project, TimedPostInput { time, thread: None }).await?);
    }
    let mut excluded = if mode == InboxReadMode::Latest {
        Some(
            append_post_at(
                &fixture,
                &foreign,
                TimedPostInput {
                    time: "2026-10-12T10:30:00Z",
                    thread: Some(thread.clone()),
                },
            )
            .await?,
        )
    } else {
        None
    };
    if mode == InboxReadMode::Latest {
        expected.reverse();
    }
    let mut request = inbox_request(
        InboxScope::Project {
            project_id: project,
        },
        reader.clone(),
        mode,
    )?;
    request.page.limit = PageLimit::try_from(page_size)?;
    let first = client.inbox_fetch(request.clone()).await?;
    let mut seen = inbox_post_ids(&first.page.records);
    assert_eq!(
        seen,
        expected
            .iter()
            .take(usize::try_from(page_size)?)
            .cloned()
            .collect::<Vec<_>>()
    );
    let mut previous_sweep = first.page.next.sweep.ok_or("first sweep")?;
    assert_eq!(
        previous_sweep.time,
        MergeTime::Known(
            (if mode == InboxReadMode::Latest {
                "2026-10-12T10:00:00Z"
            } else {
                "2026-10-12T11:00:00Z"
            })
            .parse::<chrono::DateTime<chrono::Utc>>()?
        )
    );
    request.page.cursor = Some(first.page.next_cursor.ok_or("continuation")?);
    accepted(
        &board_read_setup::keyed_call(
            &fixture,
            "thread_watch",
            json!({"actor":reader,"thread":{"thread":thread}}),
        )
        .await?,
    );
    if mode == InboxReadMode::Unread {
        excluded = Some(
            append_post_at(
                &fixture,
                &foreign,
                TimedPostInput {
                    time: "2026-10-12T10:30:00Z",
                    thread: Some(thread),
                },
            )
            .await?,
        );
    }
    for _ in 0..3 {
        let page = client.inbox_fetch(request.clone()).await?;
        seen.extend(inbox_post_ids(&page.page.records));
        let sweep = page.page.next.sweep.ok_or("continued sweep")?;
        if mode == InboxReadMode::Latest {
            assert!(sweep <= previous_sweep);
        } else {
            assert!(sweep >= previous_sweep);
        }
        previous_sweep = sweep;
        request.page.cursor = page.page.next_cursor;
        if request.page.cursor.is_none() {
            break;
        }
    }
    assert!(request.page.cursor.is_none());
    assert_eq!(
        seen, expected,
        "per-project feed order and scope exclusion survive clock regression"
    );
    request.page = PageRequest::default();
    let fresh = client.inbox_fetch(request).await?;
    assert!(inbox_post_ids(&fresh.page.records).contains(&excluded.ok_or("excluded post")?));
    fixture.stop().await
}

#[tokio::test]
async fn descending_sweep_keeps_the_minimum_across_clock_regression() -> Result<(), TestError> {
    clock_regression_case(InboxReadMode::Latest, 1).await
}
#[tokio::test]
async fn ascending_sweep_keeps_the_maximum_across_clock_regression() -> Result<(), TestError> {
    clock_regression_case(InboxReadMode::Unread, 1).await
}
#[tokio::test]
async fn a_multi_record_page_keeps_its_extreme_rather_than_its_last_key() -> Result<(), TestError> {
    clock_regression_case(InboxReadMode::Latest, 2).await?;
    clock_regression_case(InboxReadMode::Unread, 2).await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn equal_time_sweep_uses_project_and_position_and_keeps_nanosecond_order()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("complete merge key").await?;
    let (first, _) = fixture.project().await?;
    let (second, _) = fixture.project().await?;
    let (lower, higher) = if first < second {
        (first, second)
    } else {
        (second, first)
    };
    let time = "2026-10-12T10:00:00Z";
    let passed = append_post_at(&fixture, &lower, TimedPostInput { time, thread: None }).await?;
    let same_project =
        append_post_at(&fixture, &lower, TimedPostInput { time, thread: None }).await?;
    let fraction = append_post_at(
        &fixture,
        &lower,
        TimedPostInput {
            time: "2026-10-12T10:00:00.000000001Z",
            thread: None,
        },
    )
    .await?;
    let same_time_higher_project =
        append_post_at(&fixture, &higher, TimedPostInput { time, thread: None }).await?;
    let store = fixture
        ._registry
        .existing_project(&lower)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let post = read
        .record(&passed.clone().into())
        .await?
        .ok_or("passed post")?;
    let position = post
        .envelope()
        .position
        .project_cursor()
        .ok_or("post position")?;
    read.finish().await?;
    let page = client
        .post_list(PostListRequest {
            scope: PostListScope::Board {
                board_id: fixture.board.clone(),
            },
            selection: PostSelection::AfterPosition {
                after: ReadPosition::Aggregate {
                    cursor: AggregateCursor {
                        per_project: Default::default(),
                        sweep: Some(MergeKey {
                            time: MergeTime::Known(time.parse()?),
                            project_id: lower,
                            position: position.position,
                        }),
                    },
                },
            },
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(
        page.page
            .records
            .iter()
            .map(|post| post.post_id.clone())
            .collect::<Vec<_>>(),
        vec![same_project, same_time_higher_project, fraction]
    );
    assert!(page.page.records.iter().all(|post| post.post_id != passed));
    fixture.stop().await
}
