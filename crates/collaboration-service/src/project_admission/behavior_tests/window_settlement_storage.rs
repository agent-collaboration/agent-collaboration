use super::native_worker_faults;
use super::*;
use interaction_storage::{SubscriptionDeliveryProgress, SubscriptionDeliveryProgressProps};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn settlement_failure_rolls_back_delivered_window_and_outcome_together()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    let post = fixture
        .post_at(human("sender")?, "atomic delivery", fixture.now)
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    let at = fixture.now + Duration::seconds(120);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    let (_, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let before = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
    native_worker_faults::execute(fixture.interactions.clone(), "CREATE TRIGGER fail_subscription_outcome BEFORE UPDATE OF payload ON subscription_delivery_progress WHEN json_extract(NEW.payload,'$.lastOutcome') IS NOT NULL BEGIN SELECT RAISE(ABORT,'injected storage failure'); END", None).await?;
    assert!(
        fixture
            .windows
            .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
            .await
            .is_err()
    );
    let failed = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
    assert_eq!(failed, before);
    assert!(failed.last_outcome().is_none());
    native_worker_faults::execute(
        fixture.interactions.clone(),
        "DROP TRIGGER fail_subscription_outcome",
        None,
    )
    .await?;
    fixture
        .windows
        .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    let settled = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
    assert!(settled.roots().first().ok_or("root")?.window.is_none());
    assert_eq!(
        settled.last_outcome(),
        Some(&SubscriptionDeliveryOutcome::Accepted)
    );
    assert_eq!(
        Some(settled.roots().first().ok_or("root")?.delivered_through),
        post.position.project_cursor()
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn rescan_reopens_a_missing_window_from_pending_activity() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    fixture
        .post_at(human("sender")?, "pending without window", fixture.now)
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let mut props: SubscriptionDeliveryProgressProps = fixture
        .progress(&reader, &scope)
        .await?
        .ok_or("progress")?
        .into();
    for root in &mut props.roots {
        root.window = None;
    }
    fixture
        .interactions
        .put_subscription_progress(&SubscriptionDeliveryProgress::new(props)?)
        .await?;
    assert_eq!(fixture.rescan(&reader, fixture.now).await?, 1);
    let record = fixture
        .subscription(&reader, &scope, fixture.now)
        .await?
        .ok_or("subscription")?;
    assert_eq!(record.roots().len(), 1);
    assert_eq!(
        record.roots().first().ok_or("root")?.opened_at(),
        fixture.now
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn m7_rescan_prunes_a_window_with_no_pending_posts() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    let post = fixture
        .post_at(
            human("review-m7-author")?,
            "the window will become empty",
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .rescan(&reader, fixture.now + Duration::seconds(1))
        .await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let original = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
    // Fault injection at the actual local persistence boundary: delivery advances while a
    // stale pending window survives. The atomic domain record must reject this overlap.
    let RecordPlacement::Project(cursor) = post.position else {
        return Err("post cursor".into());
    };
    let cursor = cursor.cursor;
    let corrupted = native_worker_faults::execute(fixture.interactions.clone(), "UPDATE subscription_delivery_progress SET payload=json_set(payload,'$.roots[0].deliveredThrough.position',?)", Some(i64::try_from(cursor.position.get())?)).await?;
    assert_eq!(
        corrupted, 1,
        "the selected native progress row must be corrupted"
    );
    let error = fixture
        .windows
        .rescan_missing_subscription_windows(&reader, fixture.now + Duration::seconds(2))
        .await
        .expect_err("invalid combined progress must be rejected, not normalized");
    assert_eq!(error.kind, BoardFailureKind::InvalidRecord);
    // Restore the uncorrupted selected-domain record, then prove ordinary settlement
    // and rescan eliminate an empty window without re-offering the delivered post.
    fixture
        .interactions
        .put_subscription_progress(&original)
        .await?;
    let at = fixture.now + Duration::seconds(121);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    let (_, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    fixture
        .windows
        .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    fixture.rescan(&reader, at).await?;
    assert!(
        fixture
            .subscription(&reader, &scope, fixture.now + Duration::seconds(2))
            .await?
            .ok_or("subscription")?
            .roots()
            .is_empty()
    );
    fixture.stop().await
}
