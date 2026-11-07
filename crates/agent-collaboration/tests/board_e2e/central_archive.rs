//! BC03: archive propagates through notices and copied records stay readable.
use super::{
    central_pair::{SecondHost, await_view, settled_view, work_view},
    central_replication::matching_posts,
    fixture::{BoardFixture, at, fields, header, identity},
};
use message_board_storage::{MachineNoticeProgressObservation, observe_machine_notice_progress};
use router_peer_test_support::FixtureResult;
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
    let before = await_notice_progress(&reader.directory, None).await?;
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
    let after = await_notice_progress(&reader.directory, Some(before)).await?;
    assert!(
        after > before,
        "native notice progress must advance durably"
    );
    println!("BC03 canonical handled notice progress advanced {before} -> {after}");
    let archived = work_view(reader.host()?, &fixture.project).await?;
    assert_eq!(archived.get("workstreams"), view.get("workstreams"));
    assert_eq!(archived.get("source"), Some(&json!({"kind":"readCopy"})));
    let post = archived_post(reader, &fixture.project, at(&request, "/postId")?).await?;
    let stopped = reader.stop_for_readback().await?;
    let durable = notice_position(stopped.directory())
        .await?
        .ok_or("joined Host has no canonical notice-progress row")?;
    drop(stopped);
    reader.reopen_host().await?;
    await_archived(reader, &fixture.board).await?;
    let reopened_progress = await_notice_progress(&reader.directory, None).await?;
    assert!(reopened_progress >= durable);
    println!("BC03 owning reopen retained handled progress {durable} -> {reopened_progress}");
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

async fn notice_position(directory: &Path) -> FixtureResult<Option<u64>> {
    match observe_machine_notice_progress(&directory.join("fleet-cache.sqlite")).await? {
        MachineNoticeProgressObservation::NotYet => Ok(None),
        MachineNoticeProgressObservation::Present { handled_through } => {
            Ok(Some(handled_through.value()))
        }
    }
}

async fn await_notice_progress(directory: &Path, after: Option<u64>) -> FixtureResult<u64> {
    let mut last = None;
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            last = notice_position(directory).await?;
            if let Some(position) = last
                && after.is_none_or(|previous| position > previous)
            {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(position);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|error| {
        format!("canonical notice progress did not advance after {after:?}; last={last:?}: {error}")
    })?
}
