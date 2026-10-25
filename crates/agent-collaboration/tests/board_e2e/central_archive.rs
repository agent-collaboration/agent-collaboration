//! BC03: archive propagates through notices and copied records stay readable.
use super::{
    central_pair::{SecondHost, await_view, settled_view, work_view},
    central_replication::matching_posts,
    fixture::{BoardFixture, at, fields, header, identity},
};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

pub async fn archive(fixture: &mut BoardFixture, reader: &mut SecondHost) -> FixtureResult {
    fixture.create().await?;
    let thread = fixture.thread(&fixture.project, &fixture.owner).await?;
    let request = fixture
        .reply(&fixture.project, &thread, "BC03 readable archived content")
        .await?;
    let view = settled_view(fixture.host()?, &fixture.project).await?;
    await_view(reader.host()?, &fixture.project, at(&view, "/asOf")?).await?;
    let before = notice_position(&reader.directory)?;
    let empty = identity();
    fixture
        .accept(
            "board_create",
            fields(
                header(&fixture.owner),
                json!({
        "workspaceId":fixture.workspace,"boardId":empty,"name":"BC03 empty board",
        "description":"archive notice","kind":{"kind":"area"}}),
            )?,
        )
        .await?;
    fixture
        .accept(
            "board_archive",
            fields(header(&fixture.owner), json!({"boardId":empty}))?,
        )
        .await?;
    await_archived(reader, &empty).await?;
    fixture
        .accept(
            "board_archive",
            fields(header(&fixture.owner), json!({"boardId":fixture.board}))?,
        )
        .await?;
    await_archived(reader, &fixture.board).await?;
    let after = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let position = notice_position(&reader.directory)?;
            if position > before {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(position);
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    assert!(
        after > before,
        "native notice progress must advance durably"
    );
    let archived = work_view(reader.host()?, &fixture.project).await?;
    assert_eq!(archived.get("workstreams"), view.get("workstreams"));
    assert_eq!(archived.get("source"), Some(&json!({"kind":"readCopy"})));
    let post = archived_post(reader, &fixture.project, at(&request, "/postId")?).await?;
    let stopped = reader.stop_for_readback().await?;
    let durable = notice_position(stopped.directory())?;
    drop(stopped);
    reader.reopen_host().await?;
    await_archived(reader, &fixture.board).await?;
    assert!(notice_position(&reader.directory)? >= durable);
    await_view(reader.host()?, &fixture.project, at(&archived, "/asOf")?).await?;
    let reopened = archived_post(reader, &fixture.project, at(&request, "/postId")?).await?;
    assert_eq!(reopened, post);
    Ok(())
}

async fn archived_post(reader: &SecondHost, project: &str, post: &Value) -> FixtureResult<Value> {
    let feed = reader
        .host()?
        .mcp(
            "feed_read",
            json!({"projectId":project,
        "reader":{"kind":"anonymous"},"after":{"epoch":1,"position":0},
        "limit":100,"includeRedacted":false}),
        )
        .await?;
    let posts = matching_posts(&feed, post)?;
    assert_eq!(
        posts.len(),
        1,
        "archived copy retains exactly one literal post"
    );
    let record = posts.first().ok_or("archived post absent")?;
    assert_eq!(record.pointer("/content/record/record/postId"), Some(post));
    assert_eq!(
        record.pointer("/content/record/record/text"),
        Some(&json!("BC03 readable archived content"))
    );
    Ok((*record).clone())
}

async fn await_archived(reader: &SecondHost, board: &str) -> FixtureResult<Value> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let shown = reader
                .host()?
                .mcp("board_show", json!({"boardId":board}))
                .await?;
            if shown.pointer("/board/state") == Some(&json!("archived")) {
                return Ok(shown);
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

fn notice_position(directory: &Path) -> FixtureResult<u64> {
    let path = directory.join("machine-session/machine-notice-progress.json");
    match std::fs::read(path) {
        Ok(bytes) => {
            let progress: Value = serde_json::from_slice(&bytes)?;
            let position = at(&progress, "/handledThrough")?;
            if let Some(number) = position.as_u64() {
                return Ok(number);
            }
            Ok(position.as_str().ok_or("notice position absent")?.parse()?)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}
