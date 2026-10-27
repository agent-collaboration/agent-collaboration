#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! A genuinely unavailable project is reported without an endless empty continuation.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError};
use board_read_setup::{human_reader, inbox_post_ids, inbox_request, post_at_placement};
use message_board::*;
use serde_json::json;
use sqlx::{ConnectOptions, Connection};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn exhausted_healthy_pages_end_while_reporting_a_too_new_project() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let (unavailable, _) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&unavailable)
        .await?
        .ok_or("unavailable fixture")?;
    store.close_connections().await;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let supported = message_board_storage::PROJECT_MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .max()
        .ok_or("supported migration")?;
    let changed = sqlx::query("UPDATE _sqlx_migrations SET version=? WHERE version=?")
        .bind(supported + 1)
        .bind(supported)
        .execute(&mut connection)
        .await?;
    assert_eq!(changed.rows_affected(), 1);
    connection.close().await?;
    drop(store);
    let fixture = fixture.restart_read_fixture().await?;
    let client = fixture.api.client("finite unavailable pages").await?;
    let mut posts = Vec::new();
    for index in 0..2 {
        posts.push(post_at_placement(&fixture, Placement::Project { project_id: project.clone() }, json!({"actor":{"kind":"human","humanId":format!("page-author-{index}")},"text":"healthy"})).await?);
    }
    posts.reverse();
    let mut request = PostListRequest {
        scope: PostListScope::Board {
            board_id: fixture.board.clone(),
        },
        selection: PostSelection::Latest,
        page: PageRequest {
            limit: PageLimit::try_from(1)?,
            cursor: None,
        },
    };
    let first = client.post_list(request.clone()).await?;
    assert!(first.page.unavailable.contains(&unavailable));
    assert_eq!(
        first.page.records.first().map(|post| &post.post_id),
        posts.first()
    );
    request.page.cursor = Some(first.page.next_cursor.ok_or("continuation")?);
    let second = client.post_list(request).await?;
    assert!(second.page.unavailable.contains(&unavailable));
    assert_eq!(
        second.page.records.first().map(|post| &post.post_id),
        posts.get(1)
    );
    assert!(
        second.page.next_cursor.is_none(),
        "unavailable metadata is not another page of records"
    );
    let mut inbox = inbox_request(
        InboxScope::Board {
            board_id: fixture.board.clone(),
        },
        human_reader("page-reader")?,
        InboxReadMode::Latest,
    )?;
    inbox.page.limit = PageLimit::try_from(1)?;
    let first = client.inbox_fetch(inbox.clone()).await?;
    assert!(first.page.unavailable.contains(&unavailable));
    assert_eq!(
        inbox_post_ids(&first.page.records),
        vec![posts.first().ok_or("first")?.clone()]
    );
    inbox.page.cursor = Some(first.page.next_cursor.ok_or("inbox continuation")?);
    let second = client.inbox_fetch(inbox).await?;
    assert!(second.page.unavailable.contains(&unavailable));
    assert_eq!(
        inbox_post_ids(&second.page.records),
        vec![posts.get(1).ok_or("second")?.clone()]
    );
    assert!(second.page.next_cursor.is_none());
    fixture.stop().await
}
