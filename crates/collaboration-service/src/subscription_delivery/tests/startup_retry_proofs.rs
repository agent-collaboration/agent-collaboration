//! Startup fault controls use shared native fixture helpers.
use super::super::SubscriptionClock;
use super::owner_fixture::*;
use super::startup_fixture::*;
use message_board::*;
use sqlx::{ConnectOptions, Connection};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tracing_subscriber::layer::SubscriberExt;
#[tokio::test(start_paused = true)]
async fn early_startup_timeout_recovers_owed_range_without_reader_action() {
    let evidence = Arc::new(StartupEvidence::default());
    let subscriber = tracing_subscriber::registry().with(StartupTrace(evidence.clone()));
    let _trace = tracing::subscriber::set_default(subscriber);
    let fixture = OwnerFixture::new().await;
    let owed = fixture.post(&fixture.root, "owed before startup").await;
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let held = project.snapshot().await.unwrap();
    let runtime = fixture.runtime_with_clock(clock(&fixture)).await;
    assert!(
        evidence.timeout.load(Ordering::SeqCst),
        "the initial native membership probe must time out"
    );
    tokio::time::resume();
    held.finish().await.unwrap();
    // No post, list, wait, reconcile or command from this reader starts recovery.
    let notice = activity(&fixture, &runtime).await;
    assert_eq!(
        notice.ranges,
        vec![expected_range(
            fixture.project.clone(),
            fixture.root.clone(),
            owed
        )]
    );
    runtime.await_settled().await;
    evidence.state("finished").await;
    runtime.owner_barrier(&fixture.reader).await;
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "startup range delivered more than once"
    );
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn definitive_failure_after_a_deferred_probe_quarantines_and_ends_recovery() {
    let evidence = Arc::new(StartupEvidence::default());
    let subscriber = tracing_subscriber::registry().with(StartupTrace(evidence.clone()));
    let _trace = tracing::subscriber::set_default(subscriber);
    let fixture = OwnerFixture::new().await;
    fixture.post(&fixture.root, "owed but now corrupt").await;
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let held = project.snapshot().await.unwrap();
    let runtime = fixture.runtime_with_clock(clock(&fixture)).await;
    assert!(evidence.timeout.load(Ordering::SeqCst));
    tokio::time::resume();
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(project.path())
        .connect()
        .await
        .unwrap();
    assert_eq!(
        sqlx::query("UPDATE feed_checkpoint SET last_position=-1 WHERE singleton=1")
            .execute(&mut raw)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    raw.close().await.unwrap();
    held.finish().await.unwrap();
    evidence.state("quarantined").await;
    evidence.state("finished").await;
    assert!(
        fixture
            .registry
            .existing_project(&fixture.project)
            .await
            .is_err()
    );
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn shutdown_drains_a_pending_startup_discovery_retry() {
    let evidence = Arc::new(StartupEvidence::default());
    let subscriber = tracing_subscriber::registry().with(StartupTrace(evidence.clone()));
    let _trace = tracing::subscriber::set_default(subscriber);
    let fixture = OwnerFixture::new().await;
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let held = project.snapshot().await.unwrap();
    let runtime = fixture.runtime_with_clock(clock(&fixture)).await;
    assert!(evidence.timeout.load(Ordering::SeqCst));
    assert!(
        evidence
            .states
            .lock()
            .unwrap()
            .iter()
            .any(|state| state == "deferred"),
        "no tracked startup retry was retained"
    );
    tokio::time::resume();
    tokio::time::timeout(Duration::from_secs(5), runtime.service.shutdown())
        .await
        .expect("shutdown must drain the tracked discovery task");
    evidence.state("finished").await;
    held.finish().await.unwrap();
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.close().await;
}

#[derive(Clone, Copy)]
enum StartupSetupTiming {
    Immediate,
    LargeAutoAdvance,
}

#[tokio::test(start_paused = true)]
async fn healthy_project_delivers_while_another_startup_probe_is_deferred() {
    healthy_delivery_during_deferred_startup(StartupSetupTiming::Immediate).await;
}

#[tokio::test(start_paused = true)]
async fn startup_auto_advance_does_not_expire_the_healthy_projects_owed_notice() {
    healthy_delivery_during_deferred_startup(StartupSetupTiming::LargeAutoAdvance).await;
}

async fn healthy_delivery_during_deferred_startup(setup_timing: StartupSetupTiming) {
    let evidence = Arc::new(StartupEvidence::default());
    let subscriber = tracing_subscriber::registry().with(StartupTrace(evidence.clone()));
    let _trace = tracing::subscriber::set_default(subscriber);
    let fixture = OwnerFixture::new().await;
    let (healthy, thread, reader, cursor) = healthy_project(&fixture).await;
    let owed = fixture
        .post(&fixture.root, "Deferred project's owed range")
        .await;
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let held = project.snapshot().await.unwrap();
    let startup_clock = clock(&fixture);
    let starting_wall_time = fixture.clock.now();
    let startup = fixture.runtime_with_clock(startup_clock.clone());
    tokio::pin!(startup);
    if matches!(setup_timing, StartupSetupTiming::LargeAutoAdvance) {
        use futures_util::poll;
        // Startup is already inside its real storage work. With its caller not
        // polling it, paused Tokio advances a genuine timer through two days.
        assert!(poll!(startup.as_mut()).is_pending());
        let before = tokio::time::Instant::now();
        let setup_delay = Duration::from_secs(2 * 24 * 60 * 60);
        tokio::time::sleep(setup_delay).await;
        assert!(tokio::time::Instant::now().duration_since(before) >= setup_delay);
    }
    let runtime = startup.await;
    assert!(evidence.timeout.load(Ordering::SeqCst));
    tokio::time::resume();
    // B remains locked until A has delivered and settled. No reader command drives either delivery.
    let healthy_notice = activity(&fixture, &runtime).await;
    assert_eq!(
        healthy_notice.ranges,
        vec![expected_range(healthy, thread, cursor)]
    );
    runtime.await_settled().await;
    held.finish().await.unwrap();
    let recovered_notice = activity(&fixture, &runtime).await;
    assert_eq!(
        recovered_notice.ranges,
        vec![expected_range(
            fixture.project.clone(),
            fixture.root.clone(),
            owed
        )]
    );
    runtime.await_settled().await;
    evidence.state("finished").await;
    runtime.owner_barrier(&reader).await;
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    if matches!(setup_timing, StartupSetupTiming::LargeAutoAdvance) {
        assert_eq!(fixture.clock.now(), starting_wall_time);
        assert_eq!(startup_clock.now(), starting_wall_time);
        fixture.clock.advance_without_tokio_time(7);
        assert_eq!(
            startup_clock.now(),
            starting_wall_time + chrono::Duration::seconds(7)
        );
    }
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn deferred_discovery_preserves_a_selection_owned_by_a_reader_started_after_startup() {
    let evidence = Arc::new(StartupEvidence::default());
    let subscriber = tracing_subscriber::registry().with(StartupTrace(evidence.clone()));
    let _trace = tracing::subscriber::set_default(subscriber);
    let fixture = OwnerFixture::new().await;
    let owed = fixture
        .post(&fixture.root, "One owed range during discovery handoff")
        .await;
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut held = project.snapshot().await.unwrap();
    let membership = held
        .membership(
            &fixture.reader,
            &SubscriptionScope::thread(fixture.root.clone()),
        )
        .await
        .unwrap()
        .unwrap();
    let runtime = fixture.runtime().await;
    assert!(evidence.timeout.load(Ordering::SeqCst));
    tokio::time::resume();
    held.finish().await.unwrap();
    evidence.state("waiting").await;
    // The reader acts after startup but before the deferred retry's clock deadline.
    // Its existing owner has admitted the physical delivery; keep its receipt in flight.
    runtime
        .service
        .reconcile_reader(fixture.reader.clone())
        .await
        .unwrap();
    runtime.service.reconcile_queued().await;
    let notice = activity(&fixture, &runtime).await;
    assert_eq!(
        notice.ranges,
        vec![expected_range(
            fixture.project.clone(),
            fixture.root.clone(),
            owed
        )]
    );
    let before = fixture
        .push_store
        .lock()
        .await
        .subscription_progress(
            &fixture.project,
            &membership.envelope.id,
            membership.activated_at(),
        )
        .await
        .unwrap()
        .unwrap();
    let selection = before.roots()[0].window.as_ref().unwrap().selected.clone();
    assert!(selection.is_some());
    fixture.clock.advance_without_tokio_time(1);
    tokio::time::timeout(Duration::from_secs(5), runtime.service.reconcile_queued())
        .await
        .unwrap();
    assert!(
        !evidence
            .states
            .lock()
            .unwrap()
            .iter()
            .any(|state| state == "finished"),
        "handoff finished before its serialized owner acknowledged it"
    );
    let after = fixture
        .push_store
        .lock()
        .await
        .subscription_progress(
            &fixture.project,
            &membership.envelope.id,
            membership.activated_at(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        after.roots()[0].window.as_ref().unwrap().selected,
        selection,
        "deferred discovery revoked a live owner's selection"
    );
    runtime.await_settled().await;
    evidence.state("finished").await;
    runtime.owner_barrier(&fixture.reader).await;
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "handoff duplicated an admitted range"
    );
    runtime.close().await;
}
