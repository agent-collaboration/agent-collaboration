use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn only_reader_posts_renew_the_subscription() -> Result<(), TestError> {
    let fixture = SubscriptionFixture::joined().await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let initial = fixture
        .subscription(&fixture.reader, &scope, fixture.now)
        .await?
        .ok_or("record missing")?;
    fixture
        .post_at(
            human("other-reader")?,
            "other post",
            fixture.now + Duration::minutes(5),
        )
        .await?;
    assert_eq!(
        fixture
            .subscription(&fixture.reader, &scope, fixture.now + Duration::minutes(5))
            .await?
            .ok_or("record missing")?
            .renewed_at(),
        initial.renewed_at()
    );
    let reader_post_at = fixture.now + Duration::minutes(10);
    fixture
        .post_at(fixture.reader.clone(), "reader post", reader_post_at)
        .await?;
    assert_eq!(
        fixture
            .subscription(&fixture.reader, &scope, reader_post_at)
            .await?
            .ok_or("record missing")?
            .renewed_at(),
        reader_post_at
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn explicit_renewal_extends_policy_lifetime_and_off_rows_still_expire()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let renewed_at = fixture.now + Duration::hours(1);
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadRenew {
                scope: scope.clone(),
            },
            renewed_at,
        )
        .await?;
    let renewed = fixture
        .subscription(&fixture.reader, &scope, renewed_at)
        .await?
        .ok_or("record missing")?;
    assert_eq!(renewed.renewed_at(), renewed_at);
    assert_eq!(
        renewed.expires_at() - renewed.renewed_at(),
        Duration::hours(24)
    );
    assert_eq!(
        fixture
            .windows
            .list_reader_subscriptions(&fixture.reader, renewed_at)
            .await?
            .records
            .len(),
        1
    );
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: scope.clone(),
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Off),
                    ..SubscriptionPolicyPatch::default()
                },
            },
            renewed_at,
        )
        .await?;
    let expired_at = renewed_at + Duration::hours(25);
    assert!(
        fixture
            .windows
            .list_reader_subscriptions(&fixture.reader, expired_at)
            .await?
            .records
            .is_empty()
    );
    assert_eq!(
        fixture
            .subscription(&fixture.reader, &scope, expired_at)
            .await?
            .ok_or("record missing")?
            .state(),
        SubscriptionState::Ended {
            reason: EndReason::Expired
        }
    );
    let mut read = fixture.project_store().await?.snapshot().await?;
    let membership = read
        .subscription_memberships(Some(&fixture.reader))
        .await?
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|membership| membership.scope() == &scope)
        .ok_or("membership missing")?;
    assert!(
        membership.ended().is_none(),
        "expiry is derived, not an automated replicated write"
    );
    read.finish().await?;
    fixture.stop().await
}
