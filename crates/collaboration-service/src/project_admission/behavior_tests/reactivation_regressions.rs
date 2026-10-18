use super::membership_completion::join_operation;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn old_settlement_after_cancel_and_resubscribe_preserves_replacement_window_and_outcome()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let old_post = fixture
        .post_at(human("first-author")?, "old batch", fixture.now)
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    let at = fixture.now + Duration::seconds(120);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    let (_, old_selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    let old_progress = fixture
        .progress(&reader, &scope)
        .await?
        .ok_or("old progress")?;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadUnsubscribe {
                scope: scope.clone(),
            },
            at + Duration::seconds(1),
        )
        .await?;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: scope.clone(),
                policy: SubscriptionPolicyPatch::default(),
            },
            at + Duration::seconds(2),
        )
        .await?;
    let replacement = fixture
        .post_at(
            human("second-author")?,
            "replacement batch",
            at + Duration::seconds(3),
        )
        .await?;
    fixture.rescan(&reader, at + Duration::seconds(3)).await?;
    let replacement_at = at + Duration::seconds(123);
    let roots = fixture
        .windows
        .due_subscription_roots(&reader, replacement_at)
        .await?;
    let (_, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, replacement_at, usize::MAX)
        .await?;
    let retry_at = replacement_at + Duration::seconds(30);
    fixture
        .windows
        .mark_subscription_batch_retry(
            &reader,
            &selection,
            replacement_at,
            retry_at,
            SubscriptionDeliveryOutcome::Rejected {
                evidence: serde_json::json!({"receipt":"replacement"}),
            },
        )
        .await?;
    let before = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
    fixture
        .windows
        .settle_subscription_batch(
            &reader,
            &old_selection,
            SubscriptionDeliveryOutcome::Accepted,
        )
        .await?;
    let after = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
    assert_eq!(
        after, before,
        "late settlement must preserve every replacement window/outcome fact"
    );
    let record = fixture
        .subscription(&reader, &scope, replacement_at)
        .await?
        .ok_or("subscription")?;
    assert_eq!(record.roots().len(), 1);
    assert_eq!(record.roots().first().ok_or("root")?.pending_count(), 1);
    assert_eq!(
        record.roots().first().ok_or("root")?.next_retry_at(),
        Some(retry_at)
    );
    assert!(matches!(
        record.last_outcome(),
        Some(SubscriptionDeliveryOutcome::Rejected { .. })
    ));
    assert_ne!(after.activated_at(), old_progress.activated_at());
    let preserved = fixture
        .automation
        .lock()
        .await
        .subscription_progress(
            &fixture.project,
            old_progress.membership(),
            old_progress.activated_at(),
        )
        .await?
        .ok_or("old activation progress")?;
    assert_eq!(preserved, old_progress);
    let RecordPosition::Project(old_cursor) = old_post.position else {
        return Err("old position".into());
    };
    let RecordPosition::Project(replacement_cursor) = replacement.position else {
        return Err("replacement position".into());
    };
    assert!(replacement_cursor.position > old_cursor.position);
    assert!(
        after
            .roots()
            .first()
            .ok_or("root")?
            .delivered_through
            .position
            > old_cursor.position,
        "reactivation baseline follows the old activity under the newer activation binding"
    );
    fixture.stop().await
}
use super::*;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn p3_late_settlement_after_rejoin_cannot_cross_the_new_watch_boundary()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    let first = fixture
        .post_at(human("review-p3-author")?, "before leave", fixture.now)
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    let due = fixture.now + Duration::seconds(121);
    let roots = fixture.windows.due_subscription_roots(&reader, due).await?;
    let (_, old) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, due, usize::MAX)
        .await?;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadLeave {
                thread_id: fixture.root.clone(),
                after_leave: None,
            },
            due + Duration::seconds(1),
        )
        .await?;
    fixture
        .post_at(
            human("review-p3-author")?,
            "while away",
            due + Duration::seconds(2),
        )
        .await?;
    fixture
        .admit(
            reader.clone(),
            join_operation(&fixture.root, ParticipantRole::Participant),
            due + Duration::seconds(3),
        )
        .await?;
    let after = fixture
        .post_at(
            human("review-p3-author")?,
            "after rejoin",
            due + Duration::seconds(4),
        )
        .await?;
    fixture.rescan(&reader, due + Duration::seconds(4)).await?;
    fixture
        .windows
        .settle_subscription_batch(&reader, &old, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    let at = due + Duration::seconds(400);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    assert_eq!(roots, vec![fixture.root.clone()]);
    let (notice, _) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    let row = notice.roots.first().ok_or("root")?;
    assert_eq!(RecordPosition::Project(row.from), after.position);
    assert_eq!(RecordPosition::Project(row.through), after.position);
    let RecordPosition::Project(first) = first.position else {
        return Err("post position".into());
    };
    assert!(row.from.position > first.position);
    fixture.unread_posts(reader.clone()).await?;
    fixture
        .post_at(
            reader,
            "reader post after rejoin",
            due + Duration::seconds(401),
        )
        .await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn p8_rejoin_after_unsubscribe_starts_after_activity_while_unsubscribed()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    let scope = SubscriptionScope::thread(fixture.root.clone());
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadUnsubscribe {
                scope: scope.clone(),
            },
            fixture.now + Duration::seconds(1),
        )
        .await?;
    for offset in 0..3 {
        fixture
            .post_at(
                human("review-p8-author")?,
                &format!("while unsubscribed {offset}"),
                fixture.now + Duration::seconds(2 + offset),
            )
            .await?;
    }
    fixture
        .admit(
            reader.clone(),
            join_operation(&fixture.root, ParticipantRole::Participant),
            fixture.now + Duration::seconds(10),
        )
        .await?;
    fixture
        .windows
        .restore_active_and_draining(fixture.now + Duration::seconds(11))
        .await?;
    assert!(
        fixture
            .windows
            .due_subscription_roots(&reader, fixture.now + Duration::seconds(1000))
            .await?
            .is_empty()
    );
    assert!(
        fixture
            .subscription(&reader, &scope, fixture.now + Duration::seconds(1000))
            .await?
            .ok_or("subscription")?
            .roots()
            .is_empty()
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn p8_first_join_by_existing_watcher_starts_after_prior_activity() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let reader = fixture.reader.clone();
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadWatch {
                thread_id: fixture.root.clone(),
            },
            fixture.now,
        )
        .await?;
    fixture
        .post_at(
            human("review-p8-author")?,
            "before participant join",
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .admit(
            reader.clone(),
            join_operation(&fixture.root, ParticipantRole::Participant),
            fixture.now + Duration::seconds(2),
        )
        .await?;
    assert!(
        fixture
            .windows
            .due_subscription_roots(&reader, fixture.now + Duration::seconds(200))
            .await?
            .is_empty()
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn q1_rejoin_while_active_keeps_pending_window() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    fixture
        .post_at(
            human("review-q1-author")?,
            "pending before active rejoin",
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .rescan(&reader, fixture.now + Duration::seconds(1))
        .await?;
    let at = fixture.now + Duration::seconds(200);
    assert_eq!(
        fixture.windows.due_subscription_roots(&reader, at).await?,
        vec![fixture.root.clone()]
    );
    let before = fixture
        .progress(&reader, &SubscriptionScope::thread(fixture.root.clone()))
        .await?
        .ok_or("progress")?;
    fixture
        .admit(
            reader.clone(),
            join_operation(&fixture.root, ParticipantRole::Participant),
            fixture.now + Duration::seconds(30),
        )
        .await?;
    assert_eq!(
        fixture.windows.due_subscription_roots(&reader, at).await?,
        vec![fixture.root.clone()]
    );
    assert_eq!(
        fixture
            .progress(&reader, &SubscriptionScope::thread(fixture.root.clone()))
            .await?
            .ok_or("progress")?
            .roots(),
        before.roots()
    );
    fixture.stop().await
}
