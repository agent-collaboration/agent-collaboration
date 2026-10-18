#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use serde_json::json;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Cross-project references and independent scoped read marks survive archive filtering and actual reopening.
async fn cross_project_references_and_unread_summaries_keep_independent_scope()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("cross-project reader proof").await?;
    let (_, first_board, first) = create_location(&fixture, "First").await?;
    let (second_workspace, _, second) = create_location(&fixture, "Second").await?;
    let reader = human_reader("reader")?;
    for project in [&first, &second] {
        assert!(
            client
                .inbox_fetch(initialized_request(project.clone(), reader.clone())?)
                .await?
                .page
                .records
                .is_empty()
        );
    }
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: first.clone(),
        },
        json!({"text":"First project's finding"}),
    )
    .await?;
    let linked = post_at_placement(&fixture, Placement::Project { project_id: second.clone() }, json!({"text":"Second project's linked finding","references":[{"kind":"post","postId":root},{"kind":"thread","threadId":ThreadId::from(root.clone())}]})).await?;
    assert_eq!(
        Vec::from(
            client
                .post_show(PostShowRequest {
                    post_id: linked.clone()
                })
                .await?
                .0
                .references
        )
        .len(),
        2
    );
    let summaries = InboxWorkspacesRequest {
        reader: reader.clone(),
        unread_only: true,
        page: PageRequest::default(),
    };
    assert_eq!(
        client
            .inbox_workspaces(summaries.clone())
            .await?
            .page
            .records
            .len(),
        2
    );
    let first_unread = client
        .inbox_fetch(inbox_request(
            InboxScope::Project {
                project_id: first.clone(),
            },
            reader.clone(),
            InboxReadMode::Unread,
        )?)
        .await?;
    assert_eq!(
        inbox_post_ids(&first_unread.page.records),
        vec![root.clone()]
    );
    let through = first_unread
        .page
        .high_water
        .get(&first)
        .ok_or("first head")?;
    let ack = fixture.api.call("inbox_acknowledge", json!({"protocolVersion":1,"actor":reader,"actingFor":null,"entries":[{"requestId":RequestId::generate(),"resubmits":null,"observedEpoch":1,"scope":{"kind":"project","projectId":first},"through":through}]})).await?;
    assert_eq!(
        ack.pointer("/result/outcomes/0/outcome/reply/result/decision/kind"),
        Some(&json!("accepted"))
    );
    let remaining = client.inbox_workspaces(summaries.clone()).await?;
    assert_eq!(remaining.page.records.len(), 1);
    assert_eq!(
        remaining
            .page
            .records
            .first()
            .ok_or("summary")?
            .workspace_id,
        second_workspace
    );
    accepted(&keyed_call(&fixture, "board_archive", json!({"boardId":first_board})).await?);
    let reply = post_at_placement(&fixture, Placement::Thread { thread_id: ThreadId::from(linked.clone()) }, json!({"text":"Archived evidence remains referenceable","references":[{"kind":"post","postId":root}]})).await?;
    assert_eq!(
        Vec::from(
            client
                .post_show(PostShowRequest {
                    post_id: reply.clone()
                })
                .await?
                .0
                .references
        )
        .len(),
        1
    );
    let latest = client
        .post_list(PostListRequest {
            scope: PostListScope::AllWorkspaces,
            selection: PostSelection::Latest,
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(
        latest
            .page
            .records
            .iter()
            .map(|post| post.post_id.clone())
            .collect::<Vec<_>>(),
        vec![linked, root.clone()]
    );
    assert_eq!(
        latest.page.records.first().ok_or("latest root")?.project_id,
        second
    );
    assert_eq!(
        latest
            .page
            .records
            .get(1)
            .ok_or("archived root")?
            .project_id,
        first
    );
    let times = latest
        .page
        .records
        .iter()
        .map(|post| post.envelope.router_time)
        .collect::<Vec<_>>();
    assert!(times.first() >= times.get(1));
    drop(client);
    let fixture = fixture.restart_read_fixture().await?;
    // The real Host reopens project handles through creation recovery before
    // serving bare post/thread references; this fixture starts only the API.
    fixture._admission.recover_creating_projects().await?;
    let client = fixture.api.client("reopened cross-project proof").await?;
    assert_eq!(
        client
            .post_show(PostShowRequest { post_id: root })
            .await?
            .0
            .text
            .as_str(),
        "First project's finding"
    );
    let remaining = client.inbox_workspaces(summaries).await?;
    assert_eq!(remaining.page.records.len(), 1);
    assert_eq!(
        remaining
            .page
            .records
            .first()
            .ok_or("summary")?
            .workspace_id,
        second_workspace
    );
    fixture.stop().await
}
