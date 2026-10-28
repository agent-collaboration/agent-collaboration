use super::super::{SubscriptionClock, subscription_service::OwnerObservation};
use super::owner_fixture::*;
use sqlx::Connection;
use std::sync::Arc;

#[tokio::test]
async fn a_failed_scan_returns_typed_unavailable_at_the_pending_waiters_deadline() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let clock = Arc::new(RuntimeTimerClock::new(fixture.clock.clone()));
    let runtime = fixture.runtime_with_clock(clock).await;
    let mut observer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    sqlx::query("ALTER TABLE router_pushes RENAME TO blocked_router_pushes")
        .execute(&mut observer)
        .await
        .unwrap();
    observer.close().await.unwrap();
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        runtime.service.wait(
            fixture.reader.clone(),
            super::super::SubscriptionWaitFilter::All,
            1,
            usize::MAX,
        ),
    )
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(10), runtime.close())
        .await
        .unwrap();
    let refusal = response
        .expect("a failing scan stranded its pending waiter beyond the deadline")
        .expect_err("an unsuccessful scan must not report no new activity");
    assert_eq!(
        refusal.kind,
        message_board::BoardFailureKind::BoardUnavailable
    );
    assert!(refusal.message.contains("scan"), "{refusal:?}");
}

#[tokio::test]
async fn runtime_timer_clock_reaches_due_selection_and_prepared_delivery() {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let fixture = Box::pin(OwnerFixture::new()).await;
        let clock = Arc::new(RuntimeTimerClock::new(fixture.clock.clone()));
        let runtime = fixture.native_runtime_with_clock(clock.clone()).await;
        fixture
            .post_at(&fixture.root, "Timeout-free original clock", clock.now())
            .await;
        let mut observed = Vec::new();
        {
            let mut events = runtime.observations.lock().await;
            loop {
                let event = events.recv().await.unwrap();
                let due = matches!(event, OwnerObservation::DueRoots(count) if count > 0);
                observed.push(event);
                if due {
                    break;
                }
            }
        }
        assert!(
            observed
                .iter()
                .any(|event| matches!(event, OwnerObservation::RowsLoaded(1)))
        );
        println!("timeout-free owner observations: {observed:?}");
        let request = runtime.requests.lock().await.recv().await.unwrap();
        assert!(
            request
                .payload
                .line
                .as_str()
                .starts_with("🧵 Router: new thread activity")
        );
        runtime.await_settled().await;
        runtime.close().await;
    })
    .await
    .expect("bounded runtime-timer native allocation and delivery");
}

#[tokio::test(start_paused = true)]
async fn virtual_timeout_can_expire_while_the_due_query_waits_for_a_native_project_reader() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture.post(&fixture.root, "Due before query").await;
    fixture
        .store
        .lock()
        .await
        .rescan_missing_subscription_windows(&fixture.reader, fixture.clock.now())
        .await
        .unwrap();
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let held = super::startup_fixture::exhaust_project_readers(&project).await;
    let store = fixture.store.clone();
    let reader = fixture.reader.clone();
    let now = fixture.clock.now();
    let (started, started_receiver) = tokio::sync::oneshot::channel();
    let mut query = tokio::spawn(async move {
        let store = store.lock().await;
        started.send(()).unwrap();
        store.due_subscription_roots(&reader, now).await
    });
    started_receiver.await.unwrap();
    let before = tokio::time::Instant::now();
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(1), &mut query).await;
    assert!(timeout.is_err());
    assert!(!query.is_finished());
    println!(
        "virtual timer elapsed {:?} while SQLite query was still pending",
        tokio::time::Instant::now().duration_since(before)
    );
    super::startup_fixture::release_project_readers(held).await;
    tokio::time::resume();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(10), query)
            .await
            .expect("released native project readers unblock the same due query")
            .unwrap()
            .unwrap(),
        vec![fixture.root.clone()]
    );
}
