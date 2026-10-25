use super::*;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn receipts_require_exact_window_generation_and_frozen_through_before_and_after_arrivals()
-> Result<(), TestError> {
    for extended in [false, true] {
        let mut fixture = SubscriptionFixture::joined().await?;
        let reader = fixture.reader.clone();
        fixture
            .post_at(human("first-author")?, "selected range", fixture.now)
            .await?;
        fixture.rescan(&reader, fixture.now).await?;
        let at = fixture.now + Duration::seconds(120);
        let roots = vec![fixture.root.clone()];
        let (_, selection) = fixture
            .windows
            .select_subscription_notice(&reader, &roots, at, usize::MAX)
            .await?;
        if extended {
            fixture
                .post_at(
                    human("second-author")?,
                    "extended while in flight",
                    at + Duration::seconds(1),
                )
                .await?;
            fixture.rescan(&reader, at + Duration::seconds(1)).await?;
        }
        let scope = SubscriptionScope::thread(fixture.root.clone());
        let before = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
        let window = before
            .roots()
            .first()
            .ok_or("root")?
            .window
            .as_ref()
            .ok_or("window")?;
        assert_eq!(
            window.selected.as_ref().ok_or("selection")?.through,
            selection.roots.first().ok_or("receipt")?.delivered_through
        );
        if extended {
            assert!(
                window.through.position
                    > window
                        .selected
                        .as_ref()
                        .ok_or("selection")?
                        .through
                        .position
            );
        }
        let mut receipts = Vec::new();
        let mut wrong = selection.clone();
        wrong.roots.first_mut().ok_or("receipt")?.window_id = SubscriptionWindowId::generate();
        receipts.push(wrong);
        let mut wrong = selection.clone();
        wrong
            .roots
            .first_mut()
            .ok_or("receipt")?
            .subscription_generation = wrong
            .roots
            .first()
            .ok_or("receipt")?
            .subscription_generation
            .next()?;
        receipts.push(wrong);
        let mut wrong = selection.clone();
        let through = &mut wrong.roots.first_mut().ok_or("receipt")?.delivered_through;
        through.position = FeedPosition::try_from(through.position.get() + 1)?;
        receipts.push(wrong);
        let mut wrong = selection.clone();
        wrong
            .roots
            .first_mut()
            .ok_or("receipt")?
            .delivered_through
            .epoch = EpochNumber::try_from(2)?;
        receipts.push(wrong);
        for wrong in receipts {
            fixture
                .windows
                .release_subscription_batch(&reader, &wrong)
                .await?;
            fixture
                .windows
                .settle_subscription_batch(&reader, &wrong, SubscriptionDeliveryOutcome::Accepted)
                .await?;
            assert_eq!(
                fixture.progress(&reader, &scope).await?.ok_or("progress")?,
                before,
                "wrong receipts have no effect"
            );
        }
        fixture
            .windows
            .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
            .await?;
        let after = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
        assert_eq!(
            after.roots().first().ok_or("root")?.delivered_through,
            selection.roots.first().ok_or("receipt")?.delivered_through
        );
        assert!(
            after
                .roots()
                .first()
                .ok_or("root")?
                .delivered_through
                .position
                >= before
                    .roots()
                    .first()
                    .ok_or("root")?
                    .delivered_through
                    .position
        );
        fixture.rescan(&reader, at + Duration::seconds(1)).await?;
        let record = fixture
            .subscription(&reader, &scope, at + Duration::seconds(1))
            .await?
            .ok_or("subscription")?;
        assert_eq!(record.roots().len(), usize::from(extended));
        fixture.stop().await?;
    }
    Ok(())
}
