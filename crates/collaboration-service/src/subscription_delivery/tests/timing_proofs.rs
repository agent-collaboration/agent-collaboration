use router_clock::RuntimeClock;

use super::owner_fixture::*;
use crate::subscription_delivery::subscription_service::OwnerObservation;
use message_board::{SubscriptionMode, WhenIdle};

#[tokio::test]
async fn quiet_period_restarts_from_the_second_arrival() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture
        .policy(
            &fixture.root,
            SubscriptionMode::Deliver,
            WhenIdle::Hold,
            120,
            900,
        )
        .await;
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "First").await;
    runtime.owner_barrier(&fixture.reader).await;
    runtime.observe(|event| matches!(event, OwnerObservation::Sleeping(Some(deadline)) if *deadline == fixture.clock.monotonic_now() + std::time::Duration::from_secs(120))).await;
    fixture
        .clock
        .not_due_check_now(fixture.clock.monotonic_now() + std::time::Duration::from_secs(120))
        .await;
    fixture
        .clock
        .advance(std::time::Duration::from_secs(60))
        .unwrap();
    fixture.post(&fixture.root, "Second").await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture
        .clock
        .advance(std::time::Duration::from_secs(119))
        .unwrap();
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    fixture
        .clock
        .advance(std::time::Duration::from_secs(1))
        .unwrap();
    let request = runtime.requests.lock().await.recv().await.unwrap();
    assert!(request.payload.line.as_str().contains("2 messages"));
    runtime.await_settled().await;
    runtime.close().await;
}

#[tokio::test]
async fn repeated_ninety_second_arrivals_are_passed_at_the_fifteen_minute_cap() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture
        .policy(
            &fixture.root,
            SubscriptionMode::Deliver,
            WhenIdle::Hold,
            120,
            900,
        )
        .await;
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "Opening").await;
    runtime.owner_barrier(&fixture.reader).await;
    for _ in 0..9 {
        fixture
            .clock
            .advance(std::time::Duration::from_secs(90))
            .unwrap();
        fixture.post(&fixture.root, "Continuing").await;
        runtime.owner_barrier(&fixture.reader).await;
    }
    fixture
        .clock
        .advance(std::time::Duration::from_secs(89))
        .unwrap();
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    fixture
        .clock
        .advance(std::time::Duration::from_secs(1))
        .unwrap();
    let request = runtime.requests.lock().await.recv().await.unwrap();
    assert!(request.payload.line.as_str().contains("10 messages"));
    runtime.await_settled().await;
    runtime.close().await;
}

#[tokio::test]
async fn activity_in_one_root_never_delays_another_root() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let other = fixture.add_root().await;
    for root in [&fixture.root, &other] {
        fixture
            .policy(root, SubscriptionMode::Deliver, WhenIdle::Hold, 120, 900)
            .await;
    }
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "First root").await;
    fixture.post(&other, "Other root").await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture
        .clock
        .advance(std::time::Duration::from_secs(60))
        .unwrap();
    fixture.post(&other, "Other extended").await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture
        .clock
        .advance(std::time::Duration::from_secs(60))
        .unwrap();
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
    fixture
        .clock
        .advance(std::time::Duration::from_secs(60))
        .unwrap();
    runtime.requests.lock().await.recv().await.unwrap();
    runtime.await_settled().await;
    runtime.close().await;
}

#[tokio::test]
async fn rejected_delivery_backoff_doubles_from_thirty_seconds_to_ten_minutes() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "Pending").await;
    let mut request = (async { runtime.requests.lock().await.recv().await.unwrap() }).await;
    let push = request.payload.push_id.clone();
    let mut driving: Option<tokio::task::JoinHandle<Result<(), durable_steps::StoreError>>> = None;
    for (index, delay) in [30, 60, 120, 240, 480, 600, 600].into_iter().enumerate() {
        eprintln!(
            "BACKOFF answer index={index} delay={delay} now={}",
            fixture.clock.utc_now()
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
            (driving).await.unwrap().unwrap();
        }
        (async {
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
        .await;
        let until = chrono::DateTime::from_timestamp_millis(
            fixture.clock.utc_now().timestamp_millis() + i64::try_from(delay * 1000).unwrap(),
        )
        .unwrap();
        assert_eq!(
            fixture
                .public_subscription_view(&runtime)
                .await
                .next_retry_at,
            Some(until)
        );
        fixture
            .clock
            .advance(std::time::Duration::from_secs(delay - 1))
            .unwrap();
        eprintln!("BACKOFF before-deadline drive index={index}");
        let mut early_drive = runtime.drive_native_due();
        let mut early_request = None;
        (async {
            tokio::select! {
                result = &mut early_drive => { result.unwrap().unwrap(); },
                request = async { runtime.requests.lock().await.recv().await.unwrap() } => {
                    early_request = Some(request);
                    runtime.completions.send(collaboration_protocol::DeliveryOutcome::Started).unwrap();
                    early_drive.await.unwrap().unwrap();
                }
            }
        }).await;
        if let Some(early) = early_request {
            eprintln!(
                "EARLY_NATIVE_REQUEST index={index} push={} now={} scheduled_until={until}",
                early.payload.push_id.as_str(),
                fixture.clock.utc_now()
            );
            fixture
                .await_push_state(&push, collaboration_protocol::PushDeliveryState::Delivered)
                .await;
            runtime.close().await;
            panic!("known failure backoff dispatched before its scheduled time at index {index}");
        }
        eprintln!("BACKOFF before-deadline owner barrier index={index}");
        (runtime.owner_barrier(&fixture.reader)).await;
        assert!(
            runtime.requests.lock().await.try_recv().is_err(),
            "retry before its deadline"
        );
        fixture
            .clock
            .advance(std::time::Duration::from_secs(1))
            .unwrap();
        driving = Some(runtime.drive_native_due());
        request = (async { runtime.requests.lock().await.recv().await.unwrap() }).await;
    }
    assert_eq!(request.payload.push_id, push);
    runtime.await_settled().await;
    driving.unwrap().await.unwrap().unwrap();
    let delivered = fixture.public_subscription_view(&runtime).await;
    assert_eq!(delivered.next_retry_at, None);
    assert_eq!(delivered.pending_count, 0);
    runtime.close().await;
}
