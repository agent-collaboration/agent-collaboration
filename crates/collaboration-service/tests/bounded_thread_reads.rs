//! Thread pages read their workspace and returned records only.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};

async fn create_thread(fixture: &BoardFixture, project: &ProjectId) -> Result<ThreadId, TestError> {
    let post = PostId::generate();
    let mut request = fixture.header();
    request.as_object_mut().ok_or("header")?.extend(
        json!({"projectId":project,"postId":post,"about":fixture.discussion_subject(project).await?,"text":"scoped page",
            "actor":{"kind":"human","humanId":PostId::generate()},
            "references":[],"role":null,"watch":false,"addressees":[]})
        .as_object()
        .ok_or("fields")?
        .clone(),
    );
    accepted(&fixture.api.call("thread_create", request).await?);
    Ok(ThreadId::from(post))
}

async fn call_with_fields(
    fixture: &BoardFixture,
    name: &str,
    fields: Value,
) -> Result<(), TestError> {
    let mut request = fixture.header();
    request
        .as_object_mut()
        .ok_or("header")?
        .extend(fields.as_object().ok_or("fields")?.clone());
    accepted(&fixture.api.call(name, request).await?);
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn thread_page_never_decodes_another_workspaces_corrupt_thread() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let healthy = create_thread(&fixture, &project).await?;
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let foreign_project = ProjectId::generate();
    call_with_fields(
        &fixture,
        "workspace_create",
        json!({"workspaceId":workspace,"name":"other","description":""}),
    )
    .await?;
    call_with_fields(&fixture, "board_create", json!({"workspaceId":workspace,"boardId":board,"name":"other","description":"","kind":{"kind":"area"}})).await?;
    call_with_fields(
        &fixture,
        "project_create",
        json!({"boardId":board,"projectId":foreign_project,"title":"other"}),
    )
    .await?;
    let corrupt = create_thread(&fixture, &foreign_project).await?;
    let store = fixture
        ._registry
        .existing_project(&foreign_project)
        .await?
        .ok_or("foreign project")?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    sqlx::query("UPDATE record_identities SET payload = '{\"corrupt\":true}' WHERE kind = 'thread' AND id = ?")
        .bind(corrupt.as_str()).execute(&mut observer).await?;
    let page = fixture
        .api
        .call(
            "thread_list",
            json!({"workspaceId":fixture.workspace,
        "reader":fixture.actor,"watchedOnly":false,"page":{"limit":1,"cursor":null}}),
        )
        .await?;
    assert_eq!(
        page.pointer("/result/page/records/0/threadId"),
        Some(&json!(healthy)),
        "{page}"
    );
    assert_eq!(
        page.pointer("/result/page/records")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    let mut read = store.snapshot().await?;
    assert!(
        read.record(&corrupt.into()).await.is_err(),
        "direct reads still validate corrupt bodies"
    );
    read.finish().await?;
    observer.close().await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn watched_keyset_honors_thread_opt_out_and_validates_the_start_position()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let first = create_thread(&fixture, &project).await?;
    let second = create_thread(&fixture, &project).await?;
    call_with_fields(&fixture, "project_watch", json!({"projectId":project})).await?;
    call_with_fields(
        &fixture,
        "thread_unwatch",
        json!({"thread":{"thread":first}}),
    )
    .await?;
    let request = json!({"workspaceId":fixture.workspace,"reader":fixture.actor,"watchedOnly":true,
        "page":{"limit":1,"cursor":null}});
    let page = fixture.api.call("thread_list", request.clone()).await?;
    assert_eq!(
        page.pointer("/result/page/records/0/threadId"),
        Some(&json!(second)),
        "{page}"
    );
    assert_eq!(
        page.pointer("/result/page/nextCursor"),
        Some(&json!(null)),
        "{page}"
    );
    call_with_fields(
        &fixture,
        "thread_unwatch",
        json!({"thread":{"thread":second}}),
    )
    .await?;
    call_with_fields(
        &fixture,
        "thread_watch",
        json!({"thread":{"thread":second}}),
    )
    .await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let head = read.high_water();
    read.finish().await?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let reader: Identity = serde_json::from_value(fixture.actor.clone())?;
    let changed = sqlx::query(
        "UPDATE thread_watches SET start_position = ? WHERE thread_id = ? AND reader = ?",
    )
    .bind(i64::try_from(head.position.get())? + 1)
    .bind(second.as_str())
    .bind(serde_json::to_string(&reader)?)
    .execute(&mut observer)
    .await?;
    assert_eq!(
        changed.rows_affected(),
        1,
        "fault injection must reach an explicit watch"
    );
    let refused = fixture.api.call("thread_list", request).await?;
    assert_eq!(
        refused.pointer("/error/data/kind"),
        Some(&json!("invalidRecord")),
        "{refused}"
    );
    observer.close().await?;
    fixture.stop().await
}
