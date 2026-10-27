//! Plain watch choices are replicated reader state, never fake entity activity.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::{human_reader, keyed_call, post_at_placement};
use message_board::*;
use serde_json::json;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn first_thread_opt_out_survives_reopen_and_later_plain_project_watch()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"unwatched root"}),
    )
    .await?;
    let thread = ThreadId::from(root);
    let reader = human_reader("opt-out reader")?;
    let opt_out = keyed_call(
        &fixture,
        "thread_unwatch",
        json!({"actor":reader,"thread":{"thread":thread}}),
    )
    .await?;
    accepted(&opt_out);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    assert!(
        !read.has_reader_interest(&reader).await?,
        "opt-out alone is not inbox membership"
    );
    read.finish().await?;
    drop(store);
    let fixture = fixture.restart_read_fixture().await?;
    accepted(
        &keyed_call(
            &fixture,
            "project_watch",
            json!({"actor":reader,"projectId":project}),
        )
        .await?,
    );
    post_at_placement(
        &fixture,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"text":"covered activity after first opt-out"}),
    )
    .await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    assert!(
        read.effective_thread_watch(&thread, &reader)
            .await?
            .is_none(),
        "a first opt-out must defeat later covered-watch materialization"
    );
    assert!(read.thread_watch(&thread, &reader).await?.is_none());
    read.finish().await?;
    accepted(
        &keyed_call(
            &fixture,
            "thread_watch",
            json!({"actor":reader,"thread":{"thread":thread}}),
        )
        .await?,
    );
    let mut read = store.snapshot().await?;
    assert!(
        read.effective_thread_watch(&thread, &reader)
            .await?
            .is_some()
    );
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn watch_ledgers_replay_without_reappending_project_or_thread() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"watch state"}),
    )
    .await?;
    let thread = ThreadId::from(root);
    let reader = human_reader("plain reader")?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let original_project = read.record(&project.clone().into()).await?;
    let original_thread = read.record(&thread.clone().into()).await?;
    read.finish().await?;
    for (tool, fields) in [
        ("project_watch", json!({"actor":reader,"projectId":project})),
        (
            "project_unwatch",
            json!({"actor":reader,"projectId":project}),
        ),
        (
            "thread_watch",
            json!({"actor":reader,"thread":{"thread":thread}}),
        ),
        (
            "thread_unwatch",
            json!({"actor":reader,"thread":{"thread":thread}}),
        ),
    ] {
        let read = store.snapshot().await?;
        let before = read.high_water();
        read.finish().await?;
        let mut request = fixture.header();
        request
            .as_object_mut()
            .ok_or("header")?
            .extend(fields.as_object().ok_or("fields")?.clone());
        let reply = fixture.api.call(tool, request.clone()).await?;
        accepted(&reply);
        assert_eq!(
            reply.pointer("/result/result/decision/effects/entries"),
            Some(&json!([])),
            "{tool}: {reply}"
        );
        assert_eq!(
            reply.pointer("/result/result/decision/effects/totalCount"),
            Some(&json!(0))
        );
        assert_eq!(
            reply.pointer("/result/result/decision/change"),
            Some(&json!("changed"))
        );
        let mut read = store.snapshot().await?;
        let after = read.high_water();
        let FeedRead::Entries { entries, .. } = read.feed_after(before, 100, true).await? else {
            return Err("feed epoch".into());
        };
        assert_eq!(
            entries.len(),
            1,
            "only the keyed ledger decision takes a feed position"
        );
        assert!(entries.iter().all(|entry| matches!(&entry.content, FeedContent::Shown(record) if matches!(record.as_ref(), Record::Request(_)))));
        assert_eq!(
            read.record(&project.clone().into()).await?,
            original_project
        );
        assert_eq!(read.record(&thread.clone().into()).await?, original_thread);
        read.finish().await?;
        let replay = fixture.api.call(tool, request).await?;
        assert_eq!(
            reply.pointer("/result/result"),
            replay.pointer("/result/result")
        );
        let read = store.snapshot().await?;
        assert_eq!(read.high_water(), after, "replay writes nothing");
        read.finish().await?;
    }
    let read = store.snapshot().await?;
    let before = read.high_water();
    read.finish().await?;
    accepted(
        &keyed_call(
            &fixture,
            "thread_subscribe",
            json!({"actor":reader,"scope":{"kind":"project","projectId":project},"policy":{"mode":"poll"}}),
        )
        .await?,
    );
    let mut read = store.snapshot().await?;
    let FeedRead::Entries { entries, .. } = read.feed_after(before, 100, true).await? else {
        return Err("feed epoch".into());
    };
    assert!(entries.iter().any(|entry| matches!(&entry.content, FeedContent::Shown(record) if matches!(record.as_ref(), Record::SubscriptionMembership(_)))));
    assert!(!entries.iter().any(|entry| matches!(&entry.content, FeedContent::Shown(record) if matches!(record.as_ref(), Record::Project(_) | Record::Thread(_)))));
    read.finish().await?;
    fixture.stop().await
}
