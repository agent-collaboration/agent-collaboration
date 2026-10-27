#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    clippy::large_futures,
    reason = "lint pass follows the crate split"
)]
//! One feed page must not decode history or visit unrelated project contents.
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
#[path = "support/read_cost_counter.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    unused_imports,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError};
use board_read_setup::*;
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::json;

async fn page_costs(size: usize) -> Result<(ReadCost, ReadCost, ReadCost), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("bounded feed proof").await?;
    let (project, _) = fixture.project().await?;
    let (_, _, unrelated) = create_location(&fixture, "unselected, unwatched").await?;
    let reader = human_reader("bounded reader")?;
    client
        .inbox_fetch(initialized_request(project.clone(), reader.clone())?)
        .await?;
    for index in 0..size {
        for target in [&project, &unrelated] {
            post_at_placement(
                &fixture,
                Placement::Project { project_id: target.clone() },
                json!({"text":format!("post {index}"),"actor":{"kind":"human","humanId":PostId::generate()}}),
            ).await?;
        }
    }
    let posts = PostListRequest {
        scope: PostListScope::Workspace {
            workspace_id: fixture.workspace.clone(),
        },
        selection: PostSelection::Latest,
        page: PageRequest {
            limit: PageLimit::try_from(5)?,
            cursor: None,
        },
    };
    let mut inbox = inbox_request(
        InboxScope::Workspace {
            workspace_id: fixture.workspace.clone(),
        },
        reader,
        InboxReadMode::Unread,
    )?;
    inbox.page.limit = PageLimit::try_from(5)?;
    client.post_list(posts.clone()).await?;
    client.inbox_fetch(inbox.clone()).await?;
    let before = current_cost();
    let page = client.post_list(posts).await?;
    let post_cost = current_cost().since(before);
    assert_eq!(page.page.records.len(), 5);
    assert!(page.page.next_cursor.is_some());
    let foreign_before = read_cost_counter::project_statements(unrelated.as_str());
    let before = current_cost();
    let page = client.inbox_fetch(inbox).await?;
    let inbox_cost = current_cost().since(before);
    let foreign_statements =
        read_cost_counter::project_statements(unrelated.as_str()) - foreign_before;
    assert_eq!(
        foreign_statements, 1,
        "unselected/unwatched project gets only its reader-first probe"
    );
    assert_eq!(page.page.records.len(), 5);
    assert!(page.page.next_cursor.is_some());
    assert!(page.page.records.iter().all(|record| matches!(record,
        InboxActivity::PostCreated { post, .. } if post.project_id == project)));
    let before = current_cost();
    let summary = client
        .inbox_workspaces(InboxWorkspacesRequest {
            reader: human_reader("bounded reader")?,
            unread_only: true,
            page: PageRequest::default(),
        })
        .await?;
    let summary_cost = current_cost().since(before);
    assert!(
        summary
            .page
            .records
            .iter()
            .any(|row| row.workspace_id == fixture.workspace && row.has_unread)
    );
    fixture.stop().await?;
    Ok((post_cost, inbox_cost, summary_cost))
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn post_and_inbox_pages_have_size_independent_costs() -> Result<(), TestError> {
    read_cost_counter::install();
    let small = page_costs(8).await?;
    let large = page_costs(80).await?;
    eprintln!("feed page costs: small={small:?} large={large:?}");
    assert_eq!(small.0, large.0, "post page decoded unreturned history");
    assert_eq!(
        small.1, large.1,
        "inbox read beyond its page or unrelated watch probe"
    );
    assert_eq!(
        small.2, large.2,
        "summary decoded content instead of answering existence"
    );
    assert_eq!(large.2.feed_entries, 0);
    assert_eq!(large.0.feed_entries, 5);
    assert_eq!(large.1.feed_entries, 5);
    assert!(large.0.statements > 0 && large.1.statements > 0);
    Ok(())
}
