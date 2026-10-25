//! Fleet mutations and owed notices commit together; only alarms perform delivery.
#![cfg(feature = "conformance")]
#![allow(clippy::panic_in_result_fn)]
#[path = "fleet_notice_outbox/archive_notices.rs"]
mod archive_notices;
#[path = "fleet_notice_outbox/reader_progress.rs"]
mod reader_progress;
#[path = "session_publication/support.rs"]
#[allow(dead_code)]
mod session_fixture;
#[path = "fleet_notice_outbox/support.rs"]
mod support;
use board_sync_protocol::{BoardNotice, BoardReadPage, FleetFeedRead};
use durable_steps::{AtomicStore, CallFailure, storage::JournalStorage};
use message_board::FleetPosition;
use std::sync::{Arc, atomic::Ordering};
use support::*;

#[tokio::test]
async fn committed_notice_debt_survives_lost_dispatch_and_owner_reopen_then_delivers_once()
-> TestResult {
    let root = tempfile::tempdir()?;
    let delivery_root = tempfile::tempdir()?;
    let delivery = Arc::new(
        session_fixture::Fixture::open_for_installation(delivery_root.path(), installation()?)
            .await?,
    );
    let io = Io::new(delivery.clone());
    let fixture = Fixture::open(root.path(), io.clone()).await?;
    assert!(
        matches!(fixture.feed(0).await?, BoardReadPage::FleetFeed(FleetFeedRead::Page {head,..}) if head.get()==0)
    );
    let create = workspace_create()?;
    let answer = fixture.decide(&create).await?;
    assert_accepted(&answer)?;
    assert!(
        io.calls()?.is_empty(),
        "inbound mutation performed Delivery I/O"
    );
    assert_eq!(notice_rows(&delivery).await?, 0);
    io.lose_before_delivery.store(true, Ordering::SeqCst);
    let first = fixture.runtime.on_alarm().await?;
    assert_eq!(first.retried.len(), 1);
    assert_eq!(io.calls()?.len(), 1);
    assert_eq!(notice_rows(&delivery).await?, 0);
    fixture.objects.close().await?;
    drop(fixture);

    let fixture = Fixture::open(root.path(), io.clone()).await?;
    io.advance();
    let pending = fixture.runtime.on_alarm().await?;
    assert_eq!(
        pending.retried.len(),
        1,
        "Delivery must append on its separate alarm before acknowledging"
    );
    assert_eq!(notice_rows(&delivery).await?, 0);
    delivery.runtime.on_alarm().await?;
    assert_eq!(
        notice_kinds(&delivery).await?,
        vec![BoardNotice::FleetFeedAdvanced {
            through: FleetPosition::try_from(2)?
        }]
    );
    io.advance();
    let completed = fixture.runtime.on_alarm().await?;
    assert_eq!(completed.completed.len(), 1);
    assert!(completed.failed.is_empty());
    let calls = io.calls()?;
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[0], calls[1],
        "retry changed the saved call identity or payload"
    );
    assert_eq!(calls[1], calls[2]);
    let id = serde_json::from_value(calls[0]["identity"]["operation"].clone())?;
    let operation = fixture
        .object
        .transaction(move |scope| Box::pin(async move { scope.operation(id).await }))
        .await?
        .ok_or("Fleet notice task disappeared")?;
    assert!(matches!(
        operation.execution,
        durable_steps::storage::OperationExecution::Completed { .. }
    ));
    assert_eq!(fixture.decide(&create).await?, answer);
    assert!(fixture.runtime.on_alarm().await?.completed.is_empty());
    assert_eq!(
        io.calls()?.len(),
        3,
        "business replay admitted a second notice task"
    );
    assert_eq!(notice_rows(&delivery).await?, 1);
    fixture.objects.close().await?;
    delivery.objects.close().await?;
    Ok(())
}

#[tokio::test]
async fn real_sql_failure_rolls_back_the_mutation_notice_task_and_wake_together() -> TestResult {
    let root = tempfile::tempdir()?;
    let delivery_root = tempfile::tempdir()?;
    let delivery = Arc::new(
        session_fixture::Fixture::open_for_installation(delivery_root.path(), installation()?)
            .await?,
    );
    let io = Io::new(delivery.clone());
    let fixture = Fixture::open(root.path(), io.clone()).await?;
    fixture.feed(0).await?;
    let create = workspace_create()?;
    fixture.host.store.armed.store(true, Ordering::SeqCst);
    assert!(matches!(
        fixture.runtime.on_call(mutation(&create)?).await,
        Err(CallFailure::AnswerLost)
    ));
    assert!(
        fixture.host.store.injected.load(Ordering::SeqCst),
        "SQL failure never reached the post-admission transaction"
    );
    assert_eq!(fixture.counts().await?, (0, 0));
    assert!(fixture.runtime.on_alarm().await?.completed.is_empty());
    assert!(
        io.calls()?.is_empty(),
        "rolled-back journal debt escaped into Delivery"
    );
    assert_eq!(notice_rows(&delivery).await?, 0);
    assert!(
        io.wakes()?.is_empty(),
        "rolled-back wake intent armed a platform timer"
    );
    // Positive retry proves the absence oracle reached the same real path.
    assert_accepted(&fixture.decide(&create).await?)?;
    fixture.deliver().await?;
    assert_eq!(fixture.counts().await?, (1, 2));
    assert_eq!(notice_rows(&delivery).await?, 1);
    fixture.objects.close().await?;
    delivery.objects.close().await?;
    Ok(())
}

#[tokio::test]
async fn actual_delivery_acknowledgement_loss_reopens_the_fleet_debt_without_a_second_notice()
-> TestResult {
    let root = tempfile::tempdir()?;
    let delivery_root = tempfile::tempdir()?;
    let delivery = Arc::new(
        session_fixture::Fixture::open_for_installation(delivery_root.path(), installation()?)
            .await?,
    );
    let io = Io::new(delivery.clone());
    let fixture = Fixture::open(root.path(), io.clone()).await?;
    fixture.feed(0).await?;
    assert_accepted(&fixture.decide(&workspace_create()?).await?)?;
    assert_eq!(fixture.runtime.on_alarm().await?.retried.len(), 1);
    delivery.runtime.on_alarm().await?;
    assert_eq!(notice_rows(&delivery).await?, 1);
    io.lose_after_acknowledgement.store(true, Ordering::SeqCst);
    io.advance();
    assert_eq!(fixture.runtime.on_alarm().await?.retried.len(), 1);
    assert!(
        !io.lose_after_acknowledgement.load(Ordering::SeqCst),
        "the real receiver never acknowledged its committed append"
    );
    fixture.objects.close().await?;
    drop(fixture);
    let fixture = Fixture::open(root.path(), io.clone()).await?;
    io.advance();
    assert_eq!(fixture.runtime.on_alarm().await?.completed.len(), 1);
    let calls = io.calls()?;
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0], calls[1]);
    assert_eq!(calls[1], calls[2]);
    assert_eq!(
        notice_rows(&delivery).await?,
        1,
        "receiver journal deduplication failed after the sender reopened"
    );
    fixture.objects.close().await?;
    delivery.objects.close().await?;
    Ok(())
}
