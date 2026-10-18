//! Corruption is refused when its record is read, without poisoning another workspace.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use serde_json::json;
use sqlx::{ConnectOptions, Connection};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn unselected_unwatched_corrupt_post_cannot_break_a_workspace_page() -> Result<(), TestError>
{
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("bounded corruption proof").await?;
    let (project, _) = fixture.project().await?;
    let (_, _, unrelated) = create_location(&fixture, "unrelated corrupt post").await?;
    let reader = human_reader("isolation reader")?;
    client
        .inbox_fetch(initialized_request(project.clone(), reader.clone())?)
        .await?;
    let healthy = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project,
        },
        json!({"text":"healthy"}),
    )
    .await?;
    let corrupt = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: unrelated.clone(),
        },
        json!({"text":"corrupt after write"}),
    )
    .await?;
    let store = fixture
        ._registry
        .existing_project(&unrelated)
        .await?
        .ok_or("unrelated store")?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    assert_eq!(sqlx::query("UPDATE project_feed SET entry = json_set(entry, '$.content.record.record.text', '') WHERE record_kind = 'post' AND record_id = ?")
        .bind(corrupt.as_str()).execute(&mut observer).await?.rows_affected(), 1);
    let page = client
        .post_list(PostListRequest {
            scope: PostListScope::Workspace {
                workspace_id: fixture.workspace.clone(),
            },
            selection: PostSelection::Latest,
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(
        page.page
            .records
            .iter()
            .map(|post| post.post_id.clone())
            .collect::<Vec<_>>(),
        vec![healthy.clone()]
    );
    let page = client
        .inbox_fetch(inbox_request(
            InboxScope::Workspace {
                workspace_id: fixture.workspace.clone(),
            },
            reader,
            InboxReadMode::Unread,
        )?)
        .await?;
    assert_eq!(inbox_post_ids(&page.page.records), vec![healthy]);
    assert!(
        client
            .post_list(PostListRequest {
                scope: PostListScope::Project {
                    project_id: unrelated
                },
                selection: PostSelection::Latest,
                page: PageRequest::default()
            })
            .await
            .is_err(),
        "reading the corrupt feed body itself must still fail"
    );
    observer.close().await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn unread_excludes_only_the_same_session_endpoint() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture
        .api
        .client("session identity metadata proof")
        .await?;
    let (project, _) = fixture.project().await?;
    let reader: Identity = serde_json::from_value(
        json!({"kind":"session","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"same-session-id"}}),
    )?;
    let writer: Identity = serde_json::from_value(
        json!({"kind":"session","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000003","endpointId":"claude-local"},"sessionId":"same-session-id"}}),
    )?;
    client
        .inbox_fetch(initialized_request(project.clone(), reader.clone())?)
        .await?;
    let other = PostId::generate();
    for (actor, post_id) in [
        (writer, other.clone()),
        (reader.clone(), PostId::generate()),
    ] {
        accepted(&keyed_call(&fixture, "thread_create", json!({"projectId":project,"actor":actor,"observedEpoch":1,"postId":post_id,"text":"session root","role":"orchestrator","watch":false,"about":fixture.discussion_subject(&project).await?,"references":[],"addressees":[]})).await?);
    }
    let page = client
        .inbox_fetch(inbox_request(
            InboxScope::Project {
                project_id: project,
            },
            reader,
            InboxReadMode::Unread,
        )?)
        .await?;
    assert_eq!(
        inbox_post_ids(&page.page.records),
        vec![other],
        "session identity includes its endpoint"
    );
    fixture.stop().await
}
