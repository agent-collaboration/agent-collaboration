//! Newly eligible projects join beyond the browse's merge position in either direction.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::{human_reader, inbox_post_ids, inbox_request, initialized_request};
use message_board::*;
use serde_json::json;
#[path = "support/timed_board_posts.rs"]
mod timed_board_posts;
use timed_board_posts::{TimedPostInput, append_post_at};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn unread_continuation_admits_a_new_watch_only_beyond_the_merge_sweep()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("ascending merge sweep").await?;
    let (selected, _) = fixture.project().await?;
    let (outside, _) = fixture.project().await?;
    let reader = human_reader("sweep-reader")?;
    client
        .inbox_fetch(initialized_request(selected.clone(), reader.clone())?)
        .await?;
    let root = append_post_at(
        &fixture,
        &outside,
        TimedPostInput {
            time: "2026-10-12T00:00:00Z",
            thread: None,
        },
    )
    .await?;
    let first_post = append_post_at(
        &fixture,
        &selected,
        TimedPostInput {
            time: "2026-10-12T00:02:00Z",
            thread: None,
        },
    )
    .await?;
    let last_post = append_post_at(
        &fixture,
        &selected,
        TimedPostInput {
            time: "2026-10-12T00:05:00Z",
            thread: None,
        },
    )
    .await?;
    let mut request = inbox_request(
        InboxScope::Project {
            project_id: selected,
        },
        reader.clone(),
        InboxReadMode::Unread,
    )?;
    request.page.limit = PageLimit::try_from(1)?;
    let first = client.inbox_fetch(request.clone()).await?;
    assert_eq!(inbox_post_ids(&first.page.records), vec![first_post]);
    request.page.cursor = Some(first.page.next_cursor.ok_or("first cursor")?);
    let thread = ThreadId::from(root);
    accepted(
        &board_read_setup::keyed_call(
            &fixture,
            "thread_watch",
            json!({"actor":reader,"thread":{"thread":thread}}),
        )
        .await?,
    );
    let behind = append_post_at(
        &fixture,
        &outside,
        TimedPostInput {
            time: "2026-10-12T00:01:00Z",
            thread: Some(thread.clone()),
        },
    )
    .await?;
    let beyond = append_post_at(
        &fixture,
        &outside,
        TimedPostInput {
            time: "2026-10-12T00:03:00Z",
            thread: Some(thread),
        },
    )
    .await?;
    let second = client.inbox_fetch(request.clone()).await?;
    assert_eq!(inbox_post_ids(&second.page.records), vec![beyond.clone()]);
    request.page.cursor = Some(second.page.next_cursor.ok_or("second cursor")?);
    let third = client.inbox_fetch(request.clone()).await?;
    assert_eq!(inbox_post_ids(&third.page.records), vec![last_post]);
    assert!(third.page.next_cursor.is_none());
    request.page = PageRequest::default();
    let fresh = client.inbox_fetch(request).await?;
    let fresh_ids = inbox_post_ids(&fresh.page.records);
    assert!(fresh_ids.contains(&behind));
    assert!(fresh_ids.contains(&beyond));
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn post_list_continuation_admits_a_project_created_mid_browse_beyond_the_sweep()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("new project merge sweep").await?;
    let (original, _) = fixture.project().await?;
    let first_post = append_post_at(
        &fixture,
        &original,
        TimedPostInput {
            time: "2026-10-12T00:02:00Z",
            thread: None,
        },
    )
    .await?;
    let last_post = append_post_at(
        &fixture,
        &original,
        TimedPostInput {
            time: "2026-10-12T00:05:00Z",
            thread: None,
        },
    )
    .await?;
    let mut request = PostListRequest {
        scope: PostListScope::Board {
            board_id: fixture.board.clone(),
        },
        selection: PostSelection::AfterPosition {
            after: ReadPosition::Aggregate {
                cursor: AggregateCursor::default(),
            },
        },
        page: PageRequest {
            limit: PageLimit::try_from(1)?,
            cursor: None,
        },
    };
    let first = client.post_list(request.clone()).await?;
    assert_eq!(
        first
            .page
            .records
            .iter()
            .map(|post| post.post_id.clone())
            .collect::<Vec<_>>(),
        vec![first_post]
    );
    request.page.cursor = Some(first.page.next_cursor.ok_or("first cursor")?);
    let (new_project, _) = fixture.project().await?;
    let behind = append_post_at(
        &fixture,
        &new_project,
        TimedPostInput {
            time: "2026-10-12T00:01:00Z",
            thread: None,
        },
    )
    .await?;
    let beyond = append_post_at(
        &fixture,
        &new_project,
        TimedPostInput {
            time: "2026-10-12T00:03:00Z",
            thread: None,
        },
    )
    .await?;
    let second = client.post_list(request.clone()).await?;
    assert_eq!(
        second
            .page
            .records
            .iter()
            .map(|post| post.post_id.clone())
            .collect::<Vec<_>>(),
        vec![beyond.clone()]
    );
    request.page.cursor = Some(second.page.next_cursor.ok_or("second cursor")?);
    let third = client.post_list(request.clone()).await?;
    assert_eq!(
        third
            .page
            .records
            .iter()
            .map(|post| post.post_id.clone())
            .collect::<Vec<_>>(),
        vec![last_post]
    );
    assert!(third.page.next_cursor.is_none());
    request.page = PageRequest::default();
    let fresh = client.post_list(request).await?;
    assert!(fresh.page.records.iter().any(|post| post.post_id == behind));
    assert!(fresh.page.records.iter().any(|post| post.post_id == beyond));
    fixture.stop().await
}
