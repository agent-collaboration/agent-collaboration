//! Thread history guidance uses a scoped metadata maximum, not all project posts.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError};
use board_read_setup::*;
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::json;
#[allow(clippy::panic_in_result_fn)]
async fn thread_cost(size: usize) -> Result<ReadCost, TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("thread metadata maximum").await?;
    let (project, _) = fixture.project().await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"root"}),
    )
    .await?;
    let thread = ThreadId::from(root);
    let reply = post_at_placement(
        &fixture,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"text":"one reply"}),
    )
    .await?;
    let reply_position = client
        .post_show(PostShowRequest { post_id: reply })
        .await?
        .0
        .position;
    for _ in 0..size {
        post_at_placement(
            &fixture,
            Placement::Project {
                project_id: project.clone(),
            },
            json!({"actor":{"kind":"human","humanId":PostId::generate()},"text":"unrelated root"}),
        )
        .await?;
    }
    let request = ThreadShowRequest {
        include_redacted: false,
        thread: (thread.clone()).into(),
        reader: ReadActor::Identified {
            reader: human_reader("history reader")?,
        },
    };
    client.thread_show(request.clone()).await?;
    let before = current_cost();
    let result = client.thread_show(request).await?;
    let cost = current_cost().since(before);
    assert_eq!(result.thread.thread_id, thread);
    let history = result
        .watch_status
        .ok_or("status")?
        .earlier_unwatched_range
        .ok_or("history")?;
    let PostSelection::Range { to, .. } = history.selection else {
        return Err("range".into());
    };
    let RecordPlacement::Project(cursor) = reply_position else {
        return Err("reply position".into());
    };
    assert_eq!(
        to,
        ReadPosition::Project {
            cursor: cursor.cursor
        }
    );
    fixture.stop().await?;
    Ok(cost)
}
#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn thread_show_does_not_decode_unrelated_project_posts() -> Result<(), TestError> {
    read_cost_counter::install();
    let small = thread_cost(8).await?;
    let large = thread_cost(80).await?;
    eprintln!("thread history costs: small={small:?}, large={large:?}");
    assert_eq!(small, large, "thread guidance read unrelated posts");
    assert!(large.records <= 5);
    Ok(())
}
