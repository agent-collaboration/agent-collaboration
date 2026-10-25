use super::*;

// Decision 910 replaces the window-owned Held mark with the actual SDK outcome
// in control_view_proofs::native_retry_observations_reach_the_public_view_and_clear_after_readiness.

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn arrivals_after_release_do_not_restart_the_original_window_cap() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    let first = fixture.now + Duration::seconds(1);
    fixture
        .post_at(human("hold-cap-author")?, "opens the window", first)
        .await?;
    fixture.rescan(&reader, first).await?;
    let at = fixture.now + Duration::seconds(122);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    let (_, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    fixture
        .post_at(
            human("hold-cap-author")?,
            "arrives while selection is in flight",
            fixture.now + Duration::seconds(2),
        )
        .await?;
    fixture.rescan(&reader, at).await?;
    fixture
        .windows
        .release_subscription_batch(&reader, &selection)
        .await?;
    let held = fixture
        .subscription(
            &reader,
            &SubscriptionScope::thread(fixture.root.clone()),
            at,
        )
        .await?
        .ok_or("subscription")?;
    assert_eq!(held.roots().first().ok_or("root")?.opened_at(), first);
    fixture
        .post_at(
            human("hold-cap-author")?,
            "continuous activity while held",
            fixture.now + Duration::seconds(550),
        )
        .await?;
    fixture
        .rescan(&reader, fixture.now + Duration::seconds(550))
        .await?;
    assert_eq!(
        fixture
            .windows
            .due_subscription_roots(&reader, first + Duration::seconds(600))
            .await?,
        vec![fixture.root.clone()]
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn drop_storage_primitive_does_not_skip_hold_policy_activity() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    fixture
        .post_at(
            human("sender")?,
            "must remain pending under hold",
            fixture.now,
        )
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let before = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
    fixture
        .windows
        .drop_subscription_roots(
            &reader,
            std::slice::from_ref(&fixture.root),
            fixture.now + Duration::seconds(1),
        )
        .await?;
    let subscription = fixture
        .subscription(&reader, &scope, fixture.now)
        .await?
        .ok_or("subscription")?;
    assert_eq!(subscription.roots().len(), 1);
    assert_eq!(
        subscription.roots().first().ok_or("root")?.pending_count(),
        1
    );
    assert_eq!(
        fixture.progress(&reader, &scope).await?.ok_or("progress")?,
        before
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn late_lower_settlement_never_moves_delivered_backwards() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    let first = fixture
        .post_at(human("first-author")?, "first", fixture.now)
        .await?;
    let second = fixture
        .post_at(
            human("second-author")?,
            "second",
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .rescan(&reader, fixture.now + Duration::seconds(1))
        .await?;
    let at = fixture.now + Duration::seconds(121);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    let (_, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    assert_eq!(selection.roots.len(), 1);
    let mut older = selection.clone();
    let RecordPlacement::Project(first_cursor) = first.position else {
        return Err("post position".into());
    };
    let first_cursor = first_cursor.cursor;
    let RecordPlacement::Project(second_cursor) = second.position else {
        return Err("post position".into());
    };
    let second_cursor = second_cursor.cursor;
    older.roots.first_mut().ok_or("receipt")?.delivered_through = first_cursor;
    fixture
        .windows
        .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    fixture
        .windows
        .settle_subscription_batch(&reader, &older, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    assert!(
        fixture
            .windows
            .due_subscription_roots(&reader, at)
            .await?
            .is_empty()
    );
    assert_eq!(
        fixture
            .progress(&reader, &SubscriptionScope::thread(fixture.root.clone()))
            .await?
            .ok_or("progress")?
            .roots()
            .first()
            .ok_or("root")?
            .delivered_through,
        second_cursor
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn drop_advances_delivered_but_keeps_activity_unread() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    let scope = SubscriptionScope::thread(fixture.root.clone());
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: scope.clone(),
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Deliver),
                    when_idle: Some(WhenIdle::Drop),
                    ..Default::default()
                },
            },
            fixture.now,
        )
        .await?;
    let skipped = fixture
        .post_at(
            human("sender")?,
            "skipped while idle",
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .rescan(&reader, fixture.now + Duration::seconds(1))
        .await?;
    fixture
        .windows
        .drop_subscription_roots(
            &reader,
            std::slice::from_ref(&fixture.root),
            fixture.now + Duration::seconds(2),
        )
        .await?;
    assert!(
        fixture
            .subscription(&reader, &scope, fixture.now + Duration::seconds(2))
            .await?
            .ok_or("subscription")?
            .roots()
            .is_empty()
    );
    assert!(
        fixture
            .unread_posts(reader)
            .await?
            .contains(&skipped.post_id)
    );
    fixture.stop().await
}
