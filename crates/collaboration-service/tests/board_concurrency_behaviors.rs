#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
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
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use serde_json::{Value, json};
use std::{sync::Arc, time::Instant};
use tokio::sync::{Barrier, Mutex, Notify};

struct OrderedMutation {
    tool: &'static str,
    fields: Value,
}

async fn run_serial_order(
    fixture: &BoardFixture,
    first: OrderedMutation,
    second: OrderedMutation,
) -> Result<(Value, Value), TestError> {
    // The retained claim deliberately exercises both known serial orders. This
    // test-owned gate is the old fixture's mutex, not a replacement native lock.
    let gate = Mutex::new(());
    let locked = Notify::new();
    let release = Notify::new();
    let (first, second) = tokio::join!(
        async {
            let _guard = gate.lock().await;
            locked.notify_one();
            release.notified().await;
            keyed_call(fixture, first.tool, first.fields).await
        },
        async {
            locked.notified().await;
            release.notify_one();
            let _guard = gate.lock().await;
            keyed_call(fixture, second.tool, second.fields).await
        }
    );
    Ok((first?, second?))
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Both ordered archive/post interleavings retain typed refusal, actual post presence and archive state."
)]
async fn archive_and_post_race_observes_both_valid_serial_orders() -> Result<(), TestError> {
    for archive_first in [true, false] {
        let fixture = BoardFixture::start().await?;
        let (_, board, project) = create_location(&fixture, "archive order").await?;
        let post = PostId::generate();
        let archive = OrderedMutation {
            tool: "board_archive",
            fields: json!({"boardId":board}),
        };
        let create = OrderedMutation {
            tool: "thread_create",
            fields: json!({"postId":post,"projectId":project,"about":fixture.discussion_subject(&project).await?,"role":null,"watch":false,"text":"race","references":[],"addressees":[]}),
        };
        let (first, second) = if archive_first {
            run_serial_order(&fixture, archive, create).await?
        } else {
            run_serial_order(&fixture, create, archive).await?
        };
        accepted(&first);
        if archive_first {
            assert_eq!(
                second.pointer("/result/result/decision/reasons/0/kind"),
                Some(&json!("archived")),
                "{second}"
            );
        } else {
            accepted(&second);
        }
        let shown = fixture
            .api
            .call("post_show", json!({"postId":post}))
            .await?;
        assert_eq!(shown.get("error").is_none(), !archive_first, "{shown}");
        let archived = fixture
            .api
            .call("board_show", json!({"boardId":board}))
            .await?;
        // Fleet remains Archiving until the project's explicit ArchiveSeen report.
        assert_eq!(
            archived.pointer("/result/board/state"),
            Some(&json!("archiving"))
        );
        fixture.stop().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Both ordered resolve/post interleavings retain typed refusal and immutable post presence."
)]
async fn resolve_and_thread_post_race_observes_both_valid_serial_orders() -> Result<(), TestError> {
    for resolve_first in [true, false] {
        let fixture = BoardFixture::start().await?;
        let (_, _, project) = create_location(&fixture, "resolve order").await?;
        let root = post_at_placement(
            &fixture,
            Placement::Project {
                project_id: project,
            },
            json!({"text":"root"}),
        )
        .await?;
        let thread = ThreadId::from(root);
        let post = PostId::generate();
        let resolve = OrderedMutation {
            tool: "thread_resolve",
            fields: json!({"thread":{"thread":thread},"reason":"done"}),
        };
        let create = OrderedMutation {
            tool: "thread_post",
            fields: json!({"postId":post,"placement":{"kind":"thread","thread":{"thread":thread}},"text":"race","references":[],"addressees":[]}),
        };
        let (first, second) = if resolve_first {
            run_serial_order(&fixture, resolve, create).await?
        } else {
            run_serial_order(&fixture, create, resolve).await?
        };
        accepted(&first);
        if resolve_first {
            assert_eq!(
                second.pointer("/result/result/decision/reasons/0/kind"),
                Some(&json!("closed")),
                "{second}"
            );
        } else {
            accepted(&second);
        }
        let shown = fixture
            .api
            .call("post_show", json!({"postId":post}))
            .await?;
        assert_eq!(shown.get("error").is_none(), !resolve_first, "{shown}");
        let shown = fixture
            .api
            .call(
                "thread_show",
                json!({"thread":{"thread":thread},"reader":{"kind":"anonymous"}}),
            )
            .await?;
        assert_eq!(shown.pointer("/result/state"), Some(&json!("resolved")));
        fixture.stop().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Eight readers and sixteen writes preserve unread/summary equivalence at the same observation point."
)]
async fn concurrent_inbox_reads_preserve_summaries_and_report_latency() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (_, _, project) = create_location(&fixture, "inbox latency").await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"Concurrent coordination"}),
    )
    .await?;
    let thread = ThreadId::from(root);
    let readers = (0..8)
        .map(|index| human_reader(&format!("reader-{index}")))
        .collect::<Result<Vec<_>, _>>()?;
    let setup = fixture.api.client("concurrent reader setup").await?;
    for reader in &readers {
        setup
            .inbox_fetch(initialized_request(project.clone(), reader.clone())?)
            .await?;
        accepted(
            &keyed_call(
                &fixture,
                "thread_watch",
                json!({"actor":reader,"thread":{"thread":thread}}),
            )
            .await?,
        );
    }
    drop(setup);
    let fixture = Arc::new(fixture);
    let gate = Arc::new(Mutex::new(()));
    let barrier = Arc::new(Barrier::new(readers.len() + 1));
    let mut tasks = Vec::new();
    for reader in readers.clone() {
        let fixture = fixture.clone();
        let gate = gate.clone();
        let barrier = barrier.clone();
        let project = project.clone();
        tasks.push(tokio::spawn(async move {
            let client = fixture.api.client("concurrent inbox reader").await?;
            barrier.wait().await;
            let mut observations = Vec::new();
            for _ in 0..8 {
                let waiting = Instant::now();
                let _guard = gate.lock().await;
                let wait = waiting.elapsed().as_micros();
                let reading = Instant::now();
                let inbox = client
                    .inbox_fetch(inbox_request(
                        InboxScope::Project {
                            project_id: project.clone(),
                        },
                        reader.clone(),
                        InboxReadMode::Unread,
                    )?)
                    .await?;
                let summary = client
                    .inbox_workspaces(InboxWorkspacesRequest {
                        reader: reader.clone(),
                        unread_only: false,
                        page: PageRequest::default(),
                    })
                    .await?;
                assert_eq!(summary.page.records.len(), 1);
                assert_eq!(
                    summary.page.records.first().ok_or("summary")?.has_unread,
                    !inbox.page.records.is_empty()
                );
                observations.push((wait, reading.elapsed().as_micros()));
            }
            Ok::<_, TestError>(observations)
        }));
    }
    barrier.wait().await;
    let mut writes = Vec::new();
    let mut waits = Vec::new();
    for _ in 0..16 {
        let waiting = Instant::now();
        let _guard = gate.lock().await;
        waits.push(waiting.elapsed().as_micros());
        let writing = Instant::now();
        post_at_placement(
            &fixture,
            Placement::Thread {
                thread_id: thread.clone(),
            },
            json!({"text":"A new finding"}),
        )
        .await?;
        writes.push(writing.elapsed().as_micros());
    }
    let mut reads = Vec::new();
    for task in tasks {
        for (wait, read) in task.await?? {
            waits.push(wait);
            reads.push(read);
        }
    }
    assert_eq!(reads.len(), 64);
    assert_eq!(writes.len(), 16);
    assert_eq!(waits.len(), 80);
    let fixture = Arc::try_unwrap(fixture).map_err(|_| "reader fixture still shared")?;
    let client = fixture.api.client("final concurrent unread").await?;
    for reader in readers {
        let inbox = client
            .inbox_fetch(inbox_request(
                InboxScope::Project {
                    project_id: project.clone(),
                },
                reader,
                InboxReadMode::Unread,
            )?)
            .await?;
        assert_eq!(inbox.page.records.len(), 16);
    }
    eprintln!(
        "8 readers, 16 native writes: SDK read+summary p95={}us; admitted write p95={}us; test observation gate wait p95={}us",
        percentile_95(&mut reads)?,
        percentile_95(&mut writes)?,
        percentile_95(&mut waits)?
    );
    fixture.stop().await
}

fn percentile_95(observations: &mut [u128]) -> Result<u128, TestError> {
    observations.sort_unstable();
    observations
        .get((observations.len() * 95).div_ceil(100).saturating_sub(1))
        .copied()
        .ok_or_else(|| "no observations".into())
}
