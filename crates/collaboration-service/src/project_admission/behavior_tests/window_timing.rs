use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; Result propagates fixture errors."
)]
async fn post_windows_exclude_author_and_project_roots_keep_independent_timing()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    fixture
        .post_at(reader.clone(), "own post", fixture.now)
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    assert!(
        fixture
            .subscription(
                &reader,
                &SubscriptionScope::thread(fixture.root.clone()),
                fixture.now
            )
            .await?
            .ok_or("subscription")?
            .roots()
            .is_empty()
    );
    fixture.stop().await?;
    let mut fixture = SubscriptionFixture::start().await?;
    let reader = session("project-window-reader")?;
    let scope = SubscriptionScope::project(fixture.project.clone());
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: scope.clone(),
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Poll),
                    ..Default::default()
                },
            },
            fixture.now,
        )
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    fixture
        .post_at(
            human("first-author")?,
            "first root activity",
            fixture.now + Duration::seconds(10),
        )
        .await?;
    let second = ThreadId::generate();
    fixture
        .create_root_at(
            &second,
            human("root-author")?,
            "second root",
            fixture.now + Duration::seconds(30),
        )
        .await?;
    fixture
        .post_in_thread_at(
            &second,
            human("second-author")?,
            "second root activity",
            fixture.now + Duration::seconds(40),
        )
        .await?;
    let due = fixture.now + Duration::seconds(130);
    fixture.rescan(&reader, due).await?;
    let record = fixture
        .subscription(&reader, &scope, due)
        .await?
        .ok_or("subscription")?;
    assert_eq!(record.roots().len(), 2);
    assert_eq!(
        record
            .roots()
            .iter()
            .find(|row| row.thread_id() == &fixture.root)
            .ok_or("first")?
            .opened_at(),
        fixture.now + Duration::seconds(10)
    );
    assert_eq!(
        record
            .roots()
            .iter()
            .find(|row| row.thread_id() == &second)
            .ok_or("second")?
            .opened_at(),
        fixture.now + Duration::seconds(30)
    );
    assert_eq!(
        fixture.windows.due_subscription_roots(&reader, due).await?,
        vec![fixture.root.clone()]
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; Result propagates fixture errors."
)]
async fn residual_arrival_opens_at_first_arrival_while_batch_is_in_flight() -> Result<(), TestError>
{
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    fixture
        .post_at(human("first-author")?, "initial activity", fixture.now)
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    let at = fixture.now + Duration::seconds(120);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    let (notice, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    assert_eq!(notice.roots.len(), 1);
    let arrival = fixture.now + Duration::seconds(130);
    fixture
        .post_at(human("second-author")?, "residual activity", arrival)
        .await?;
    fixture.rescan(&reader, arrival).await?;
    fixture
        .windows
        .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    fixture.rescan(&reader, arrival).await?;
    let record = fixture
        .subscription(
            &reader,
            &SubscriptionScope::thread(fixture.root.clone()),
            arrival,
        )
        .await?
        .ok_or("subscription")?;
    assert_eq!(record.roots().len(), 1);
    let root = record.roots().first().ok_or("root")?;
    assert_eq!(root.opened_at(), arrival);
    assert_eq!(root.last_arrival_at(), arrival);
    assert_eq!(root.pending_count(), 1);
    assert!(
        fixture
            .windows
            .due_subscription_roots(&reader, arrival + Duration::seconds(119))
            .await?
            .is_empty()
    );
    assert_eq!(
        fixture
            .windows
            .due_subscription_roots(&reader, arrival + Duration::seconds(120))
            .await?,
        vec![fixture.root.clone()]
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; Result propagates fixture errors."
)]
async fn residual_window_that_reaches_its_cap_is_due_immediately_after_settlement()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    let scope = SubscriptionScope::thread(fixture.root.clone());
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: scope.clone(),
                policy: SubscriptionPolicyPatch {
                    timing: SubscriptionTimingPatch {
                        quiet_seconds: Some(120),
                        cap_seconds: Some(600),
                    },
                    ..Default::default()
                },
            },
            fixture.now,
        )
        .await?;
    fixture
        .post_at(human("first-author")?, "initial activity", fixture.now)
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    let at = fixture.now + Duration::seconds(600);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    let (_, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    let first = fixture.now + Duration::seconds(610);
    let last = fixture.now + Duration::seconds(1211);
    fixture
        .post_at(human("second-author")?, "first residual", first)
        .await?;
    fixture
        .post_at(human("third-author")?, "late residual", last)
        .await?;
    fixture.rescan(&reader, last).await?;
    fixture
        .windows
        .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    fixture.rescan(&reader, last).await?;
    let record = fixture
        .subscription(&reader, &scope, last)
        .await?
        .ok_or("subscription")?;
    let root = record.roots().first().ok_or("root")?;
    assert_eq!(root.opened_at(), first);
    assert_eq!(root.last_arrival_at(), last);
    assert_eq!(
        fixture
            .windows
            .due_subscription_roots(&reader, fixture.now + Duration::seconds(1212))
            .await?,
        vec![fixture.root.clone()]
    );
    fixture.stop().await
}
