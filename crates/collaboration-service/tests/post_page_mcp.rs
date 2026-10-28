//! Aggregate post paging keeps escape-heavy records within the existing byte budget.
use message_board::*;
use serde_json::json;
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
#[path = "support/board_read_setup.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod board_read_setup;
use board_read_setup::{human_reader, inbox_post_ids, inbox_request, initialized_request};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn escaped_post_pages_traverse_every_record_under_the_frame_budget() -> Result<(), TestError>
{
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let root = PostId::generate();
    let thread = ThreadId::from(root.clone());
    let reader = human_reader("byte-budget-reader")?;
    let client = fixture.api.client("post-byte-budget").await?;
    let mut expected = vec![root.clone()];
    for index in 0..10 {
        let id = if index == 0 {
            root.clone()
        } else {
            PostId::generate()
        };
        let mut input = fixture.header();
        input["postId"] = json!(id);
        let tool = if index == 0 {
            input["projectId"] = json!(project);
            input["about"] = json!({"kind":"project","subject":{"line":"Byte-budget root"}});
            input["role"] = json!(null);
            input["watch"] = json!(false);
            "thread_create"
        } else {
            input["placement"] = json!({"kind":"thread","thread":{"thread":thread}});
            "thread_post"
        };
        input["text"] = json!(if index == 0 {
            "root".to_owned()
        } else {
            "\\\"".repeat(32_768)
        });
        input["references"] = json!([]);
        input["addressees"] = json!([]);
        accepted(&fixture.api.call(tool, input).await?);
        if index == 0 {
            assert!(
                client
                    .inbox_fetch(initialized_request(project.clone(), reader.clone())?)
                    .await?
                    .page
                    .records
                    .is_empty()
            );
            let mut watch = fixture.header();
            watch["actor"] = json!(reader);
            watch["thread"] = json!({"thread":thread});
            accepted(&fixture.api.call("thread_watch", watch).await?);
        }
        if index != 0 {
            expected.push(id);
        }
    }
    expected.reverse();
    let mut next = None;
    let mut observed = Vec::new();
    let mut pages = 0;
    loop {
        let result = client
            .post_list(PostListRequest {
                scope: PostListScope::Board {
                    board_id: fixture.board.clone(),
                },
                selection: PostSelection::Latest,
                page: PageRequest {
                    limit: PageLimit::try_from(100)?,
                    cursor: next,
                },
            })
            .await?;
        assert!(
            serde_json::to_vec(&result)?.len() < collaboration_protocol::MAX_CONTROL_FRAME_BYTES
        );
        for post in &result.page.records {
            assert_eq!(
                post.text.as_str(),
                if post.post_id == root {
                    "root".to_owned()
                } else {
                    "\\\"".repeat(32_768)
                }
            );
        }
        observed.extend(result.page.records.into_iter().map(|post| post.post_id));
        pages += 1;
        next = result.page.next_cursor;
        if next.is_none() {
            break;
        }
    }
    assert_eq!(
        observed, expected,
        "byte-limited cursor traversal skipped/repeated a post"
    );
    assert!(pages >= 2);
    let mut request = inbox_request(
        InboxScope::Project {
            project_id: project,
        },
        reader,
        InboxReadMode::Unread,
    )?;
    request.page.limit = PageLimit::try_from(100)?;
    let mut inbox_ids = Vec::new();
    let mut inbox_pages = 0;
    loop {
        let result = client.inbox_fetch(request.clone()).await?;
        assert!(
            serde_json::to_vec(&result)?.len() < collaboration_protocol::MAX_CONTROL_FRAME_BYTES
        );
        for activity in &result.page.records {
            if let InboxActivity::PostCreated { post, .. } = activity {
                assert_eq!(post.text.as_str(), "\\\"".repeat(32_768));
            }
        }
        inbox_ids.extend(inbox_post_ids(&result.page.records));
        inbox_pages += 1;
        request.page.cursor = result.page.next_cursor;
        if request.page.cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        inbox_ids,
        expected
            .into_iter()
            .rev()
            .filter(|post| *post != root)
            .collect::<Vec<_>>()
    );
    assert!(inbox_pages >= 2);
    fixture.stop().await
}
