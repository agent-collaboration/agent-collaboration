use super::*;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn release_subscription_restores_due_without_advancing_delivered() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    fixture
        .post_at(human("writer")?, "Pending", fixture.now)
        .await?;
    let reader = fixture.reader.clone();
    fixture.rescan(&reader, fixture.now).await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let before = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
    let due = fixture.now + Duration::seconds(120);
    let roots = vec![fixture.root.clone()];
    let (_, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, due, usize::MAX)
        .await?;
    assert!(
        fixture
            .windows
            .due_subscription_roots(&reader, due)
            .await?
            .is_empty()
    );
    fixture
        .windows
        .release_subscription_batch(&reader, &selection)
        .await?;
    assert_eq!(
        fixture.windows.due_subscription_roots(&reader, due).await?,
        roots
    );
    let record = fixture
        .subscription(&reader, &scope, due)
        .await?
        .ok_or("subscription")?;
    assert_eq!(record.roots().first().ok_or("root")?.pending_count(), 1);
    let after = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
    assert_eq!(
        after.roots().first().ok_or("root")?.delivered_through,
        before.roots().first().ok_or("root")?.delivered_through
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn release_subscription_stale_window_or_through_fence_is_a_noop() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    fixture
        .post_at(human("writer")?, "Pending", fixture.now)
        .await?;
    let reader = fixture.reader.clone();
    fixture.rescan(&reader, fixture.now).await?;
    let due = fixture.now + Duration::seconds(120);
    let (_, selection) = fixture
        .windows
        .select_subscription_notice(
            &reader,
            std::slice::from_ref(&fixture.root),
            due,
            usize::MAX,
        )
        .await?;
    let mut stale = selection.clone();
    stale.roots.first_mut().ok_or("root")?.window_id = SubscriptionWindowId::generate();
    fixture
        .windows
        .release_subscription_batch(&reader, &stale)
        .await?;
    assert!(
        fixture
            .windows
            .due_subscription_roots(&reader, due)
            .await?
            .is_empty()
    );
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let before = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
    let mut stale = selection.clone();
    let through = &mut stale.roots.first_mut().ok_or("root")?.delivered_through;
    through.position = FeedPosition::try_from(through.position.get() + 1)?;
    fixture
        .windows
        .release_subscription_batch(&reader, &stale)
        .await?;
    assert_eq!(
        fixture.progress(&reader, &scope).await?.ok_or("progress")?,
        before,
        "a mismatched selected through must leave the selection fenced"
    );
    assert!(
        fixture
            .windows
            .due_subscription_roots(&reader, due)
            .await?
            .is_empty()
    );
    fixture
        .windows
        .release_subscription_batch(&reader, &selection)
        .await?;
    assert_eq!(
        fixture.windows.due_subscription_roots(&reader, due).await?,
        vec![fixture.root.clone()]
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn release_subscription_preserves_opened_held_and_retry_facts() -> Result<(), TestError> {
    for retry in [false, true] {
        let mut fixture = SubscriptionFixture::joined().await?;
        fixture
            .post_at(human("writer")?, "Pending", fixture.now)
            .await?;
        let reader = fixture.reader.clone();
        fixture.rescan(&reader, fixture.now).await?;
        let selected_at = fixture.now + Duration::seconds(120);
        let roots = vec![fixture.root.clone()];
        let (_, selection) = fixture
            .windows
            .select_subscription_notice(&reader, &roots, selected_at, usize::MAX)
            .await?;
        let outcome = SubscriptionDeliveryOutcome::NotSubmitted {
            retryable: true,
            reason: "hold".to_owned(),
        };
        if retry {
            fixture
                .windows
                .mark_subscription_batch_retry(
                    &reader,
                    &selection,
                    selected_at,
                    selected_at + Duration::seconds(30),
                    outcome,
                )
                .await?;
        } else {
            fixture
                .windows
                .mark_subscription_batch_held(&reader, &selection, selected_at, outcome)
                .await?;
        }
        let scope = SubscriptionScope::thread(fixture.root.clone());
        let before = fixture
            .subscription(&reader, &scope, selected_at)
            .await?
            .ok_or("subscription")?;
        let (_, selection) = fixture
            .windows
            .select_subscription_notice(
                &reader,
                &roots,
                selected_at + Duration::seconds(30),
                usize::MAX,
            )
            .await?;
        fixture
            .post_at(
                human("writer")?,
                "Arrives in flight",
                selected_at + Duration::seconds(31),
            )
            .await?;
        fixture
            .rescan(&reader, selected_at + Duration::seconds(31))
            .await?;
        fixture
            .windows
            .release_subscription_batch(&reader, &selection)
            .await?;
        let after = fixture
            .subscription(&reader, &scope, selected_at + Duration::seconds(31))
            .await?
            .ok_or("subscription")?;
        let before_root = before.roots().first().ok_or("root")?;
        let after_root = after.roots().first().ok_or("root")?;
        assert_eq!(after_root.opened_at(), before_root.opened_at());
        assert_eq!(after_root.held_since(), before_root.held_since());
        assert_eq!(after_root.next_retry_at(), before_root.next_retry_at());
        assert_eq!(after_root.retry_attempts(), before_root.retry_attempts());
        assert_eq!(after.last_outcome(), before.last_outcome());
        assert_eq!(after_root.pending_count(), 2);
        let progress = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
        assert!(
            progress
                .roots()
                .first()
                .ok_or("root")?
                .window
                .as_ref()
                .ok_or("window")?
                .selected
                .is_none()
        );
        fixture.stop().await?;
    }
    Ok(())
}
