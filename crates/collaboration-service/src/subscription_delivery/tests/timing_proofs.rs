use super::super::SubscriptionClock;
use super::owner_fixture::*;
use crate::service_identity::subscription_delivery::subscription_service::OwnerObservation;
use message_board::{SubscriptionMode, WhenIdle};

#[tokio::test(start_paused = true)]
async fn quiet_period_restarts_from_the_second_arrival() {
    let fixture = OwnerFixture::new().await;
    fixture
        .policy(
            &fixture.root,
            SubscriptionMode::Deliver,
            WhenIdle::Hold,
            120,
            600,
        )
        .await;
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "First").await;
    runtime.owner_barrier(&fixture.reader).await;
    runtime.observe(|event| matches!(event, OwnerObservation::Sleeping(Some(deadline)) if *deadline == fixture.clock.monotonic_now() + std::time::Duration::from_secs(120))).await;
    fixture.clock.advance(60).await;
    fixture.post(&fixture.root, "Second").await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture.clock.advance(119).await;
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    fixture.clock.advance(1).await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    assert!(request.payload.line.as_str().contains("2 messages"));
    runtime.await_settled().await;
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn repeated_ninety_second_arrivals_are_passed_at_the_ten_minute_cap() {
    let fixture = OwnerFixture::new().await;
    fixture
        .policy(
            &fixture.root,
            SubscriptionMode::Deliver,
            WhenIdle::Hold,
            120,
            600,
        )
        .await;
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "Opening").await;
    runtime.owner_barrier(&fixture.reader).await;
    for _ in 0..6 {
        fixture.clock.advance(90).await;
        fixture.post(&fixture.root, "Continuing").await;
        runtime.owner_barrier(&fixture.reader).await;
    }
    fixture.clock.advance(59).await;
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    fixture.clock.advance(1).await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    assert!(request.payload.line.as_str().contains("7 messages"));
    runtime.await_settled().await;
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn activity_in_one_root_never_delays_another_root() {
    let fixture = OwnerFixture::new().await;
    let other = fixture.add_root().await;
    for root in [&fixture.root, &other] {
        fixture
            .policy(root, SubscriptionMode::Deliver, WhenIdle::Hold, 120, 600)
            .await;
    }
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "First root").await;
    fixture.post(&other, "Other root").await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture.clock.advance(60).await;
    fixture.post(&other, "Other extended").await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture.clock.advance(60).await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    let record = fixture
        .interactions
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.activity.unwrap().ranges[0].thread_id, fixture.root);
    runtime.await_settled().await;
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    fixture.clock.advance(60).await;
    runtime.requests.lock().await.recv().await.unwrap();
    runtime.await_settled().await;
    runtime.close().await;
}

#[tokio::test]
async fn rejected_delivery_backoff_doubles_from_thirty_seconds_to_ten_minutes() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "Pending").await;
    let mut request = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    let push = request.payload.push_id.clone();
    let mut driving: Option<tokio::task::JoinHandle<Result<(), durable_steps::StoreError>>> = None;
    for (index, delay) in [30, 60, 120, 240, 480, 600, 600].into_iter().enumerate() {
        eprintln!(
            "BACKOFF answer index={index} delay={delay} now={}",
            fixture.clock.now()
        );
        assert_eq!(
            request.payload.push_id, push,
            "backoff must keep the frozen item"
        );
        runtime
            .completions
            .send(collaboration_protocol::DeliveryOutcome::NotSubmitted {
                retryable: false,
                reason: format!("target rejects attempt {index}"),
            })
            .unwrap();
        if let Some(driving) = driving.take() {
            tokio::time::timeout(std::time::Duration::from_secs(10), driving)
                .await
                .expect("preceding retry drive must finish after this answer")
                .unwrap()
                .unwrap();
        }
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if fixture
                    .interactions
                    .local_delivery_retry_attempts(&push)
                    .await
                    .unwrap()
                    == u32::try_from(index + 1).unwrap()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("actual SDK outcome transaction recorded this retry");
        let until = chrono::DateTime::from_timestamp_millis(
            fixture.clock.now().timestamp_millis() + i64::try_from(delay * 1000).unwrap(),
        )
        .unwrap();
        assert_eq!(
            fixture
                .public_subscription_view(&runtime)
                .await
                .next_retry_at,
            Some(until)
        );
        fixture.clock.advance_without_tokio_time(delay - 1);
        eprintln!("BACKOFF before-deadline drive index={index}");
        let mut early_drive = runtime.drive_native_due();
        let mut early_request = None;
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::select! {
                result = &mut early_drive => { result.unwrap().unwrap(); },
                request = async { runtime.requests.lock().await.recv().await.unwrap() } => {
                    early_request = Some(request);
                    runtime.completions.send(collaboration_protocol::DeliveryOutcome::Started).unwrap();
                    early_drive.await.unwrap().unwrap();
                }
            }
        }).await.expect("before-deadline drive or actual unexpected native request");
        if let Some(early) = early_request {
            eprintln!(
                "EARLY_NATIVE_REQUEST index={index} push={} now={} scheduled_until={until}",
                early.payload.push_id.as_str(),
                fixture.clock.now()
            );
            fixture
                .await_push_state(&push, collaboration_protocol::PushDeliveryState::Delivered)
                .await;
            runtime.close().await;
            panic!("known failure backoff dispatched before its scheduled time at index {index}");
        }
        eprintln!("BACKOFF before-deadline owner barrier index={index}");
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            runtime.owner_barrier(&fixture.reader),
        )
        .await
        .expect("before-deadline reader must not be blocked in native IO");
        assert!(
            runtime.requests.lock().await.try_recv().is_err(),
            "retry before its deadline"
        );
        fixture.clock.advance_without_tokio_time(1);
        driving = Some(runtime.drive_native_due());
        request = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            runtime.requests.lock().await.recv().await.unwrap()
        })
        .await
        .expect("retry at its actual SDK deadline");
    }
    assert_eq!(request.payload.push_id, push);
    runtime.await_settled().await;
    driving.unwrap().await.unwrap().unwrap();
    let delivered = fixture.public_subscription_view(&runtime).await;
    assert_eq!(delivered.next_retry_at, None);
    assert_eq!(delivered.pending_count, 0);
    runtime.close().await;
}
