#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Combined native startup states must use normal restoration and acknowledged ownership.
use super::super::SubscriptionClock;
use super::super::tests::{owner_fixture::*, startup_fixture::*};
use super::*;
use message_board::*;
use sqlx::{ConnectOptions, Connection};
use std::{sync::atomic::Ordering, time::Duration};
use tracing_subscriber::layer::SubscriberExt;

#[tokio::test(start_paused = true)]
async fn deferred_project_restores_an_abandoned_selection_despite_the_same_readers_healthy_owner() {
    let evidence = Arc::new(StartupEvidence::default());
    let _trace = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(StartupTrace(evidence.clone())),
    );
    let fixture = OwnerFixture::new().await;
    let (healthy, healthy_thread, _, healthy_owed) =
        subscribed_project(&fixture, fixture.reader.clone()).await;
    let owed = fixture
        .post(&fixture.root, "selected before process restart")
        .await;
    let mut windows = fixture.store.lock().await;
    windows
        .rescan_missing_subscription_windows(&fixture.reader, fixture.clock.now())
        .await
        .unwrap();
    let (batch, settlement) = windows
        .select_subscription_notice(
            &fixture.reader,
            std::slice::from_ref(&fixture.root),
            fixture.clock.now(),
            usize::MAX,
        )
        .await
        .unwrap();
    assert_eq!(batch.roots.len(), 1);
    assert_eq!(settlement.roots[0].delivered_through, owed);
    drop(windows);
    let mut automation = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM router_pushes")
            .fetch_one(&mut automation)
            .await
            .unwrap(),
        0,
        "the crash precedes push preparation"
    );
    automation.close().await.unwrap();
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut held = exhaust_project_readers(&project).await;
    evidence.timeout.store(false, Ordering::SeqCst);
    let before = held[0]
        .membership(
            &fixture.reader,
            &SubscriptionScope::thread(fixture.root.clone()),
        )
        .await
        .unwrap()
        .unwrap();
    let runtime = fixture.native_runtime_with_clock(clock(&fixture)).await;
    assert!(evidence.timeout.load(Ordering::SeqCst));
    tokio::time::resume();
    let notice = activity(&fixture, &runtime).await;
    assert_eq!(
        notice.ranges,
        vec![expected_range(healthy, healthy_thread, healthy_owed)]
    );
    runtime.await_settled().await;
    release_project_readers(held).await;
    // No reader action: A's existing owner must not protect B's pre-crash selection.
    evidence.state("finished").await;
    let restored = fixture
        .interactions
        .subscription_progress(&fixture.project, &before.envelope.id, before.activated_at())
        .await
        .unwrap()
        .unwrap();
    assert!(
        restored.roots()[0]
            .window
            .as_ref()
            .and_then(|window| window.selected.as_ref())
            .is_none_or(|selected| selected.batch_id != batch.batch_id),
        "healthy A owner protected B's abandoned pre-crash selection"
    );
    let recovered = activity(&fixture, &runtime).await;
    assert_eq!(
        recovered.ranges,
        vec![expected_range(
            fixture.project.clone(),
            fixture.root.clone(),
            owed
        )]
    );
    runtime.await_settled().await;
    evidence.state("finished").await;
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    let mut read = project.snapshot().await.unwrap();
    let after = read
        .membership(
            &fixture.reader,
            &SubscriptionScope::thread(fixture.root.clone()),
        )
        .await
        .unwrap()
        .unwrap();
    read.finish().await.unwrap();
    assert_eq!(after.activated_at(), before.activated_at());
    assert_eq!(after.generation(), before.generation());
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn deferred_handoff_replaces_an_owner_that_accepts_then_drops_its_command() {
    let evidence = Arc::new(StartupEvidence::default());
    let _trace = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(StartupTrace(evidence.clone())),
    );
    let fixture = OwnerFixture::new().await;
    let owed = fixture
        .post(&fixture.root, "owed after closing owner's accepted handoff")
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
    evidence.state("waiting").await;
    let (stale_sender, mut stale_receiver) = mpsc::channel(1);
    runtime
        .service
        .inner
        .owners
        .lock()
        .await
        .insert(fixture.reader.clone(), stale_sender);
    release_project_readers(held).await;
    fixture.clock.advance_without_tokio_time(1);
    let accepted = tokio::time::timeout(Duration::from_secs(5), stale_receiver.recv())
        .await
        .unwrap()
        .unwrap();
    let ReaderDeliveryCommand::Reconcile { scope, reply, .. } = accepted else {
        panic!("scoped reconciliation handoff")
    };
    assert!(
        matches!(scope,ReaderScanScope::Projects(ref projects) if projects==&std::collections::BTreeSet::from([fixture.project.clone()]))
    );
    stale_receiver.close();
    drop(reply);
    drop(stale_receiver);
    // Acceptance was real, but only the replacement can take responsibility.
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
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn terminal_project_removal_survives_a_later_shared_failure_across_retry_passes() {
    let evidence = Arc::new(StartupEvidence::default());
    let _trace = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(StartupTrace(evidence.clone())),
    );
    let fixture = OwnerFixture::new().await;
    let (recovering, thread, reader, owed) = healthy_project(&fixture).await;
    let broken = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let healthy = fixture
        .registry
        .existing_project(&recovering)
        .await
        .unwrap()
        .unwrap();
    let held_broken = exhaust_project_readers(&broken).await;
    let held_healthy = exhaust_project_readers(&healthy).await;
    evidence.timeout.store(false, Ordering::SeqCst);
    let runtime = fixture.native_runtime().await;
    assert!(evidence.timeout.load(Ordering::SeqCst));
    tokio::time::resume();
    evidence.state("waiting").await;
    let mut passes = runtime.service.inner.startup_passes.subscribe();
    let mut native = sqlx_turso::TursoConnectOptions::new()
        .filename(broken.path())
        .connect()
        .await
        .unwrap();
    assert_eq!(
        sqlx::query("UPDATE project_feed SET entry_id='corrupt-startup-head' WHERE position=?")
            .bind(i64::try_from(held_broken[0].high_water().position.get()).unwrap())
            .execute(&mut native)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    native.close().await.unwrap();
    let mut automation = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    sqlx::query("ALTER TABLE subscription_delivery_progress RENAME TO unavailable_progress")
        .execute(&mut automation)
        .await
        .unwrap();
    release_project_readers(held_broken).await;
    release_project_readers(held_healthy).await;
    let mut pending = Vec::new();
    for _ in 0..3 {
        fixture.clock.advance_without_tokio_time(1);
        pending.push(
            tokio::time::timeout(Duration::from_secs(5), passes.recv())
                .await
                .unwrap()
                .unwrap(),
        );
    }
    assert_eq!(
        pending,
        vec![vec![recovering.clone()]; 3],
        "a terminal B outcome was discarded with the shared C failure"
    );
    assert_eq!(
        evidence
            .states
            .lock()
            .unwrap()
            .iter()
            .filter(|state| state.as_str() == "quarantined")
            .count(),
        1,
        "terminal project logged again without a transition"
    );
    assert!(
        fixture
            .registry
            .existing_project(&fixture.project)
            .await
            .is_err()
    );
    sqlx::query("ALTER TABLE unavailable_progress RENAME TO subscription_delivery_progress")
        .execute(&mut automation)
        .await
        .unwrap();
    automation.close().await.unwrap();
    fixture.clock.advance_without_tokio_time(1);
    let notice = activity(&fixture, &runtime).await;
    assert_eq!(
        notice.ranges,
        vec![expected_range(recovering, thread, owed)]
    );
    runtime.await_settled().await;
    evidence.state("finished").await;
    runtime.owner_barrier(&reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.close().await;
}

#[tokio::test]
async fn recovering_project_does_not_wait_for_an_owner_without_membership() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.native_runtime().await;
    let Identity::Session { mut session } = fixture.reader.clone() else {
        panic!("session reader")
    };
    session.session_id = SessionId::try_from("unrelated-message-reader".to_owned()).unwrap();
    let unrelated = Identity::Session { session };
    // A direct-message owner can legitimately be busy without a board membership.
    // Its channel cannot acknowledge a board command until its own effect settles.
    let (unrelated_sender, mut unrelated_receiver) = mpsc::channel(1);
    runtime
        .service
        .inner
        .owners
        .lock()
        .await
        .insert(unrelated, unrelated_sender);
    let probe = fixture.store.lock().await.clone();
    let result=tokio::time::timeout(Duration::from_secs(5),async {
        tokio::select! {
            outcome=runtime.service.restore_deferred_project(&probe,&fixture.project) => outcome.map(|outcome| matches!(outcome, super::subscription_startup_recovery::StartupProjectOutcome::Restored)),
            _=unrelated_receiver.recv() => Ok(false),
        }
    }).await;
    runtime.close().await;
    assert!(
        matches!(result, Ok(Ok(true))),
        "project recovery commanded or waited for an unrelated reader owner"
    );
}
