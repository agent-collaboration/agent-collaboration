use super::super::subscription_service::OwnerObservation;
use super::owner_fixture::*;
use router_clock::RuntimeClock;
use sqlx::Connection;

#[tokio::test]
async fn a_failed_scan_returns_typed_unavailable_at_the_pending_waiters_deadline() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let clock = fixture.clock.clone();
    let runtime = fixture.runtime_with_clock(clock.clone()).await;
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
    let deadline = clock.monotonic_now() + std::time::Duration::from_secs(1);
    let service = runtime.service.clone();
    let reader = fixture.reader.clone();
    let waiting = tokio::spawn(async move {
        service
            .wait(
                reader,
                super::super::SubscriptionWaitFilter::All,
                1,
                usize::MAX,
            )
            .await
    });
    clock.not_due_check_now(deadline).await;
    clock.advance(std::time::Duration::from_secs(1)).unwrap();
    let refusal = waiting
        .await
        .unwrap()
        .expect_err("an unsuccessful scan must not report no new activity");
    runtime.close().await;
    assert_eq!(
        refusal.kind,
        message_board::BoardFailureKind::BoardUnavailable
    );
    assert!(refusal.message.contains("scan"), "{refusal:?}");
}

#[tokio::test]
async fn runtime_timer_clock_reaches_due_selection_and_prepared_delivery() {
    (async {
        let fixture = Box::pin(OwnerFixture::new()).await;
        let clock = fixture.clock.clone();
        let runtime = fixture.native_runtime_with_clock(clock.clone()).await;
        fixture
            .post_at(
                &fixture.root,
                "Timeout-free original clock",
                clock.utc_now(),
            )
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
    .await;
}

#[tokio::test]
async fn virtual_timeout_can_expire_while_the_due_query_waits_for_a_native_project_reader() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture.post(&fixture.root, "Due before query").await;
    fixture
        .store
        .lock()
        .await
        .rescan_missing_subscription_windows(&fixture.reader, fixture.clock.utc_now())
        .await
        .unwrap();
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    tokio::time::pause();
    let held = super::startup_fixture::exhaust_project_readers(&project).await;
    tokio::time::resume();
    let store = fixture.store.clone();
    let reader = fixture.reader.clone();
    let now = fixture.clock.utc_now();
    let (started, started_receiver) = tokio::sync::oneshot::channel();
    let mut query = tokio::spawn(async move {
        let store = store.lock().await;
        started.send(()).unwrap();
        store.due_subscription_roots(&reader, now).await
    });
    started_receiver.await.unwrap();
    {
        let bound = router_clock::timeout(
            fixture.clock.as_ref(),
            std::time::Duration::from_secs(1),
            &mut query,
        );
        tokio::pin!(bound);
        assert!(futures_util::poll!(&mut bound).is_pending());
        fixture
            .clock
            .advance(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(bound.await.is_err());
    }
    assert!(!query.is_finished());
    super::startup_fixture::release_project_readers(held).await;
    assert_eq!((query).await.unwrap().unwrap(), vec![fixture.root.clone()]);
}
