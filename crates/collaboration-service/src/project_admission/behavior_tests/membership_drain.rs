use super::*;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn m3_unsubscribe_after_expiry_keeps_the_expired_end_reason() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let expired = fixture.now + Duration::hours(24) + Duration::seconds(1);
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadUnsubscribe {
                scope: scope.clone(),
            },
            expired,
        )
        .await?;
    assert_eq!(
        fixture
            .subscription(&fixture.reader, &scope, expired)
            .await?
            .ok_or("subscription")?
            .state(),
        SubscriptionState::Ended {
            reason: EndReason::Expired
        }
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn m6_draining_thread_subscription_completes_as_resolved_after_settlement()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    let scope = SubscriptionScope::thread(fixture.root.clone());
    fixture
        .post_at(
            human("review-m6-author")?,
            "pending while the thread resolves",
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .rescan(&reader, fixture.now + Duration::seconds(1))
        .await?;
    fixture
        .admit(
            human("owner")?,
            BoardOperation::ThreadResolve {
                thread_id: fixture.root.clone(),
                reason: ThreadResolutionReason::try_from("done".to_owned())?,
            },
            fixture.now + Duration::seconds(2),
        )
        .await?;
    assert_eq!(
        fixture
            .subscription(&reader, &scope, fixture.now + Duration::seconds(3))
            .await?
            .ok_or("subscription")?
            .state(),
        SubscriptionState::Draining
    );
    let at = fixture.now + Duration::seconds(122);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    let (_, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    fixture
        .windows
        .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    let completed = fixture
        .subscription(&reader, &scope, at + Duration::seconds(1))
        .await?
        .ok_or("subscription")?;
    assert_eq!(
        completed.state(),
        SubscriptionState::Ended {
            reason: EndReason::Resolved
        }
    );
    assert!(completed.roots().is_empty());
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn p7_drop_policy_processes_a_draining_subscription() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    let scope = SubscriptionScope::thread(fixture.root.clone());
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: scope.clone(),
                policy: SubscriptionPolicyPatch {
                    when_idle: Some(WhenIdle::Drop),
                    ..Default::default()
                },
            },
            fixture.now,
        )
        .await?;
    fixture
        .post_at(
            human("review-p7-author")?,
            "pending before resolution",
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .rescan(&reader, fixture.now + Duration::seconds(1))
        .await?;
    fixture
        .admit(
            human("owner")?,
            BoardOperation::ThreadResolve {
                thread_id: fixture.root.clone(),
                reason: ThreadResolutionReason::try_from("done".to_owned())?,
            },
            fixture.now + Duration::seconds(2),
        )
        .await?;
    fixture
        .windows
        .drop_subscription_roots(
            &reader,
            std::slice::from_ref(&fixture.root),
            fixture.now + Duration::seconds(3),
        )
        .await?;
    let record = fixture
        .subscription(&reader, &scope, fixture.now + Duration::seconds(3))
        .await?
        .ok_or("subscription")?;
    assert!(record.roots().is_empty());
    // Completion is derived in the current model; no extra replicated completion write.
    assert_eq!(
        record.state(),
        SubscriptionState::Ended {
            reason: EndReason::Resolved
        }
    );
    fixture.stop().await
}
