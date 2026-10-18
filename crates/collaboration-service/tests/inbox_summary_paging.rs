//! A watched foreign workspace cannot hide this workspace's unread activity.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use serde_json::json;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Real summary and page assertions retain cross-workspace content.
async fn unread_summary_checks_beyond_a_full_foreign_first_page() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("summary paging proof").await?;
    let (foreign_workspace, _, foreign_project) =
        create_location(&fixture, "foreign first").await?;
    let (own_workspace, _, own_project) = create_location(&fixture, "own last").await?;
    let reader = human_reader("summary reader")?;
    for project in [&foreign_project, &own_project] {
        accepted(
            &keyed_call(
                &fixture,
                "project_watch",
                json!({"projectId":project,"actor":reader,"observedEpoch":1}),
            )
            .await?,
        );
    }
    client
        .inbox_fetch(initialized_request(own_project.clone(), reader.clone())?)
        .await?;
    let foreign_root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: foreign_project.clone(),
        },
        json!({"text":"foreign root"}),
    )
    .await?;
    for index in 0..101 {
        post_at_placement(&fixture,Placement::Thread{thread_id:ThreadId::from(foreign_root.clone())},json!({"text":format!("foreign {index}"),"actor":{"kind":"human","humanId":format!("foreign sender {index}")}})).await?;
    }
    let own_post = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: own_project.clone(),
        },
        json!({"text":"own unread"}),
    )
    .await?;
    let mut request = inbox_request(
        InboxScope::Workspace {
            workspace_id: own_workspace.clone(),
        },
        reader.clone(),
        InboxReadMode::Unread,
    )?;
    request.page.limit = PageLimit::try_from(100)?;
    let first = client.inbox_fetch(request).await?;
    assert_eq!(first.page.records.len(), 100);
    assert!(first.page.records.iter().all(|entry| matches!(entry,InboxActivity::PostCreated{workspace_id,..} if workspace_id==&foreign_workspace)));
    assert!(first.page.next_cursor.is_some());
    let summaries = client
        .inbox_workspaces(InboxWorkspacesRequest {
            reader: reader.clone(),
            unread_only: true,
            page: PageRequest::default(),
        })
        .await?;
    let own = summaries
        .page
        .records
        .iter()
        .find(|row| row.workspace_id == own_workspace)
        .ok_or("own unread workspace omitted")?;
    assert!(own.has_unread);
    let mut next = inbox_request(
        InboxScope::Workspace {
            workspace_id: own_workspace,
        },
        reader,
        InboxReadMode::Unread,
    )?;
    next.page.cursor = first.page.next_cursor;
    let second = client.inbox_fetch(next).await?;
    assert!(inbox_post_ids(&second.page.records).contains(&own_post));
    fixture.stop().await
}
