//! Startup fault controls
use super::owner_fixture::*;
use super::startup_fixture::*;
use router_clock::RuntimeClock;
use sqlx::{ConnectOptions, Connection};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tracing_subscriber::layer::SubscriberExt;
#[tokio::test]
async fn early_startup_timeout_recovers_owed_range_without_reader_action() {
    let evidence = Arc::new(StartupEvidence::default());
    let subscriber = tracing_subscriber::registry().with(StartupTrace(evidence.clone()));
    let _trace = tracing::subscriber::set_default(subscriber);
    let fixture = Box::pin(OwnerFixture::new()).await;
    let owed = fixture.post(&fixture.root, "owed before startup").await;
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    tokio::time::pause();
    let held = exhaust_project_readers(&project).await;
    evidence.timeout.store(false, Ordering::SeqCst);
    // The owner runs on the test clock: the startup retry fires when the test moves the
    // clock to its deadline, not on a real timer.
    let runtime = fixture.native_runtime().await;
    let retry_at = fixture.clock.monotonic_now() + Duration::from_secs(1);
    assert!(
        evidence.timeout.load(Ordering::SeqCst),
        "the initial native membership probe must time out"
    );
    tokio::time::resume();
    release_project_readers(held).await;
    // No post, list, wait, reconcile or command from this reader starts recovery; only
    // the retry's own deadline does.
    (fixture.clock.not_due_check_now(retry_at)).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    fixture
        .clock
        .advance(retry_at.saturating_duration_since(fixture.clock.monotonic_now()))
        .unwrap();
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

#[tokio::test]
async fn definitive_failure_after_a_deferred_probe_quarantines_and_ends_recovery() {
    let evidence = Arc::new(StartupEvidence::default());
    let subscriber = tracing_subscriber::registry().with(StartupTrace(evidence.clone()));
    let _trace = tracing::subscriber::set_default(subscriber);
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture.post(&fixture.root, "owed but now corrupt").await;
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    tokio::time::pause();
    let held = exhaust_project_readers(&project).await;
    evidence.timeout.store(false, Ordering::SeqCst);
    // The owner runs on the test clock: the startup retry fires at its deadline.
    let runtime = fixture.native_runtime().await;
    let retry_at = fixture.clock.monotonic_now() + Duration::from_secs(1);
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
    (fixture.clock.not_due_check_now(retry_at)).await;
    fixture
        .clock
        .advance(retry_at.saturating_duration_since(fixture.clock.monotonic_now()))
        .unwrap();
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

#[tokio::test]
async fn shutdown_drains_a_pending_startup_discovery_retry() {
    let evidence = Arc::new(StartupEvidence::default());
    let subscriber = tracing_subscriber::registry().with(StartupTrace(evidence.clone()));
    let _trace = tracing::subscriber::set_default(subscriber);
    let fixture = Box::pin(OwnerFixture::new()).await;
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    tokio::time::pause();
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
    (runtime.service.shutdown()).await;
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

#[tokio::test]
async fn healthy_project_delivers_while_another_startup_probe_is_deferred() {
    Box::pin(healthy_delivery_during_deferred_startup(
        StartupSetupTiming::Immediate,
    ))
    .await;
}

#[tokio::test]
async fn startup_auto_advance_does_not_expire_the_healthy_projects_owed_notice() {
    Box::pin(healthy_delivery_during_deferred_startup(
        StartupSetupTiming::LargeAutoAdvance,
    ))
    .await;
}

/// A healthy project delivers while another project's startup probe is deferred.
///
/// `Immediate` runs the owner on the test clock: the deferred project recovers only when the
/// test moves the clock to its first retry's deadline. `LargeAutoAdvance` keeps runtime
/// timers, because it proves a two-day paused Tokio advance during setup expires nothing;
/// it ends once the healthy project has settled. The deferred project's recovery beside a
/// healthy owner is proven here and in
/// `deferred_project_restores_an_abandoned_selection_despite_the_same_readers_healthy_owner`.
async fn healthy_delivery_during_deferred_startup(setup_timing: StartupSetupTiming) {
    let evidence = Arc::new(StartupEvidence::default());
    let subscriber = tracing_subscriber::registry().with(StartupTrace(evidence.clone()));
    let _trace = tracing::subscriber::set_default(subscriber);
    let fixture = Box::pin(OwnerFixture::new()).await;
    let (healthy, thread, reader, cursor) = Box::pin(healthy_project(&fixture)).await;
    let owed = fixture
        .post(&fixture.root, "Deferred project's owed range")
        .await;
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    eprintln!("startup long-setup: fixture ready");
    tokio::time::pause();
    let held = exhaust_project_readers(&project).await;
    eprintln!("startup long-setup: pool exhausted");
    evidence.timeout.store(false, Ordering::SeqCst);
    let starting_wall_time = fixture.clock.utc_now();
    let runtime_timers =
        matches!(setup_timing, StartupSetupTiming::LargeAutoAdvance).then(|| clock(&fixture));
    let (runtime, retry_at) = match &runtime_timers {
        Some(startup_clock) => {
            let startup = fixture.native_runtime_with_clock(startup_clock.clone());
            tokio::pin!(startup);
            use futures_util::poll;
            // Startup is already inside its real storage work. With its caller not
            // polling it, paused Tokio advances a genuine timer through two days.
            assert!(poll!(startup.as_mut()).is_pending());
            eprintln!("startup long-setup: startup polled; entering native pool clock jump");
            let before = tokio::time::Instant::now();
            let setup_delay = Duration::from_secs(2 * 24 * 60 * 60);
            tokio::time::sleep(setup_delay).await;
            assert!(tokio::time::Instant::now().saturating_duration_since(before) >= setup_delay);
            eprintln!("startup long-setup: native clock jumped; awaiting startup");
            (startup.await, None)
        }
        None => {
            let runtime = fixture.native_runtime().await;
            let retry_at = fixture.clock.monotonic_now() + Duration::from_secs(1);
            (runtime, Some(retry_at))
        }
    };
    assert!(evidence.timeout.load(Ordering::SeqCst));
    tokio::time::resume();
    eprintln!(
        "startup long-setup: startup returned; states={:?}",
        evidence.states.lock().unwrap()
    );
    // B remains locked until A has delivered and settled. No reader command drives either delivery.
    eprintln!("startup long-setup: waiting for healthy notice");
    let healthy_notice = activity(&fixture, &runtime).await;
    eprintln!("startup long-setup: healthy notice received");
    assert_eq!(
        healthy_notice.ranges,
        vec![expected_range(healthy.clone(), thread, cursor)]
    );
    runtime.await_settled().await;
    eprintln!("startup long-setup: healthy settlement received with broken readers held");
    // The pool's inherited timer advanced during setup; B's retry still needs its one clock.
    if runtime_timers.is_some() {
        let retry_at = fixture.clock.monotonic_now() + Duration::from_secs(1);
        eprintln!("startup long-setup: waiting for injected retry {retry_at:?}");
        fixture.clock.not_due_check_now(retry_at).await;
        eprintln!("startup long-setup: stepping injected retry");
        fixture.clock.advance(Duration::from_secs(1)).unwrap();
    }
    if let Some(startup_clock) = &runtime_timers {
        assert_eq!(
            fixture.clock.utc_now(),
            starting_wall_time + chrono::Duration::seconds(1)
        );
        assert_eq!(
            startup_clock.utc_now(),
            starting_wall_time + chrono::Duration::seconds(1)
        );
        fixture
            .clock
            .advance(std::time::Duration::from_secs(7))
            .unwrap();
        assert_eq!(
            startup_clock.utc_now(),
            starting_wall_time + chrono::Duration::seconds(8)
        );
        // A delivered and settled while B stayed held and its retry clock was frozen.
        // Releasing B now lets its already-stepped retry recover the original owed range.
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
        runtime.owner_barrier(&reader).await;
        while let Ok(request) = runtime.requests.lock().await.try_recv() {
            let push = fixture
                .interactions
                .get_push_record(&request.payload.push_id)
                .await
                .unwrap()
                .unwrap();
            assert!(
                push.activity
                    .iter()
                    .flat_map(|activity| &activity.ranges)
                    .all(|range| range.project_id != healthy),
                "A was delivered a second time"
            );
        }
    } else if let Some(retry_at) = retry_at {
        release_project_readers(held).await;
        // Only the first retry's own deadline recovers B.
        (fixture.clock.not_due_check_now(retry_at)).await;
        assert!(runtime.requests.lock().await.try_recv().is_err());
        fixture
            .clock
            .advance(retry_at.saturating_duration_since(fixture.clock.monotonic_now()))
            .unwrap();
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
        runtime.owner_barrier(&fixture.reader).await;
        runtime.owner_barrier(&reader).await;
        assert!(runtime.requests.lock().await.try_recv().is_err());
    }
    runtime.close().await;
}

#[tokio::test]
async fn deferred_discovery_preserves_a_selection_owned_by_a_reader_started_after_startup() {
    let evidence = Arc::new(StartupEvidence::default());
    let subscriber = tracing_subscriber::registry().with(StartupTrace(evidence.clone()));
    let _trace = tracing::subscriber::set_default(subscriber);
    let fixture = Box::pin(OwnerFixture::new()).await;
    let owed = fixture
        .post(&fixture.root, "One owed range during discovery handoff")
        .await;
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    tokio::time::pause();
    let held = exhaust_project_readers(&project).await;
    evidence.timeout.store(false, Ordering::SeqCst);
    let runtime = fixture.native_runtime().await;
    assert!(evidence.timeout.load(Ordering::SeqCst));
    tokio::time::resume();
    release_project_readers(held).await;
    evidence.state("waiting").await;
    (async {
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
        fixture
            .clock
            .advance(std::time::Duration::from_secs(1))
            .unwrap();
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
    .await;
}
