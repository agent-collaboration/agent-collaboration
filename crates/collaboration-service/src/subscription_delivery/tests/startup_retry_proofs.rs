//! Startup fault controls use shared native fixture helpers.
use super::super::SubscriptionClock;
use super::owner_fixture::*;
use super::startup_fixture::*;
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
    let held = exhaust_project_readers(&project).await;
    evidence.timeout.store(false, Ordering::SeqCst);
    let runtime = fixture.native_runtime_with_clock(clock(&fixture)).await;
    assert!(
        evidence.timeout.load(Ordering::SeqCst),
        "the initial native membership probe must time out"
    );
    tokio::time::resume();
    release_project_readers(held).await;
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
    let held = exhaust_project_readers(&project).await;
    evidence.timeout.store(false, Ordering::SeqCst);
    let runtime = fixture.native_runtime_with_clock(clock(&fixture)).await;
    assert!(evidence.timeout.load(Ordering::SeqCst));
    tokio::time::resume();
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(project.path())
        .connect()
        .await
        .unwrap();
    assert_eq!(
        sqlx::query("UPDATE project_feed SET entry_id='corrupt-startup-head' WHERE position=?")
            .bind(i64::try_from(held[0].high_water().position.get()).unwrap())
            .execute(&mut raw)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    raw.close().await.unwrap();
    release_project_readers(held).await;
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
    let held = exhaust_project_readers(&project).await;
    evidence.timeout.store(false, Ordering::SeqCst);
    let runtime = fixture.native_runtime_with_clock(clock(&fixture)).await;
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
    release_project_readers(held).await;
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
    let held = exhaust_project_readers(&project).await;
    evidence.timeout.store(false, Ordering::SeqCst);
    let startup_clock = clock(&fixture);
    let starting_wall_time = fixture.clock.now();
    let startup = fixture.native_runtime_with_clock(startup_clock.clone());
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
    release_project_readers(held).await;
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
    let held = exhaust_project_readers(&project).await;
    evidence.timeout.store(false, Ordering::SeqCst);
    let runtime = fixture.native_runtime().await;
    assert!(evidence.timeout.load(Ordering::SeqCst));
    tokio::time::resume();
    release_project_readers(held).await;
    evidence.state("waiting").await;
    tokio::time::timeout(Duration::from_secs(20), async {
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
        let frozen =
            super::local_mail_proofs::native_follow_mail_proofs::frozen_follow(&fixture).await;
        assert_eq!(frozen.len(), 1);
        let item = frozen[0].id();
        let reference = serde_json::to_value(&frozen[0]).unwrap();
        let board_sync_protocol::BoardDeliveryBody::Notification(body) = frozen[0].body() else {
            panic!("frozen notification");
        };
        let collaboration_mail_protocol::NotificationSource::FollowBatch { roots, .. } =
            &body.source
        else {
            panic!("follow source");
        };
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].root(), &fixture.root);
        assert_eq!(roots[0].from(), owed);
        assert_eq!(roots[0].through(), owed);
        assert_eq!(roots[0].post_count(), 1);
        let push = item.as_uuid().to_string().try_into().unwrap();
        let before = fixture
            .interactions
            .get_push_record(&push)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            before.delivery_state,
            collaboration_protocol::PushDeliveryState::Attempted
        );
        fixture.clock.advance_without_tokio_time(1);
        runtime.service.reconcile_queued().await;
        let after =
            super::local_mail_proofs::native_follow_mail_proofs::frozen_follow(&fixture).await;
        assert_eq!(
            after.len(),
            1,
            "deferred retry re-froze a covered range under a new item ID"
        );
        assert_eq!(
            serde_json::to_value(&after[0]).unwrap(),
            reference,
            "deferred retry must not re-freeze the same range under a different identity"
        );
        assert_eq!(
            serde_json::to_value(
                fixture
                    .interactions
                    .get_push_record(&push)
                    .await
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(before).unwrap(),
            "deferred retry must keep the same public Attempted push until native Started"
        );
        tokio::select! {
            () = runtime.await_settled() => {},
            duplicate = async { runtime.requests.lock().await.recv().await } => {
                panic!("deferred discovery made a second carrier request: {duplicate:?}");
            }
        }
        assert_eq!(
            runtime.await_mail_outcome(&push).await,
            fleet_sync_protocol::RecipientOutcome::Delivered {
                acceptance: collaboration_mail_protocol::CarrierAcceptance::Started,
            }
        );
        evidence.state("resolved").await;
        evidence.state("finished").await;
        let completed =
            super::local_mail_proofs::native_follow_mail_proofs::frozen_follow(&fixture).await;
        assert_eq!(completed.len(), 1);
        assert_eq!(
            serde_json::to_value(&completed[0]).unwrap(),
            reference,
            "completed deferred retry must preserve the original frozen identity and payload"
        );
        runtime.owner_barrier(&fixture.reader).await;
        assert!(
            runtime.requests.lock().await.try_recv().is_err(),
            "handoff duplicated an admitted range"
        );
        runtime.close().await;
    })
    .await
    .expect("bounded deferred startup custody and exact-item delivery");
}
