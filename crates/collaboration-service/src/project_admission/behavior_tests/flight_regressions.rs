use super::*;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn p4_policy_patch_during_flight_releases_the_matching_window() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    let scope = SubscriptionScope::thread(fixture.root.clone());
    fixture
        .post_at(human("review-p4-author")?, "selected", fixture.now)
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    let at = fixture.now + Duration::seconds(121);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    let (_, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: scope.clone(),
                policy: SubscriptionPolicyPatch {
                    timing: SubscriptionTimingPatch {
                        quiet_seconds: Some(60),
                        cap_seconds: None,
                    },
                    ..Default::default()
                },
            },
            at + Duration::seconds(1),
        )
        .await?;
    fixture
        .windows
        .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
        .await?;
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
    fixture
        .post_at(
            human("review-p4-author")?,
            "new arrival",
            at + Duration::seconds(2),
        )
        .await?;
    fixture.rescan(&reader, at + Duration::seconds(2)).await?;
    let later = at + Duration::seconds(2000);
    let roots = fixture
        .windows
        .due_subscription_roots(&reader, later)
        .await?;
    let (notice, _) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, later, usize::MAX)
        .await?;
    assert_eq!(notice.roots.len(), 1);
    assert_eq!(
        notice.roots.first().ok_or("root")?.post_count,
        2,
        "the policy-change binding preserves the old unread backlog"
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn p5_resolve_during_flight_retry_clears_in_flight_and_allows_selection()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    let scope = SubscriptionScope::thread(fixture.root.clone());
    fixture
        .post_at(human("review-p5-author")?, "selected", fixture.now)
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    let at = fixture.now + Duration::seconds(121);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    let (_, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    fixture
        .admit(
            human("owner")?,
            BoardOperation::ThreadResolve {
                thread_id: fixture.root.clone(),
                reason: ThreadResolutionReason::try_from("done".to_owned())?,
            },
            at + Duration::seconds(1),
        )
        .await?;
    fixture
        .windows
        .mark_subscription_batch_retry(
            &reader,
            &selection,
            at + Duration::seconds(2),
            at + Duration::seconds(32),
            SubscriptionDeliveryOutcome::Rejected {
                evidence: serde_json::json!({}),
            },
        )
        .await?;
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
    let later = at + Duration::seconds(2000);
    let roots = fixture
        .windows
        .due_subscription_roots(&reader, later)
        .await?;
    let (notice, _) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, later, usize::MAX)
        .await?;
    assert_eq!(notice.roots.len(), 1);
    fixture.stop().await
}
