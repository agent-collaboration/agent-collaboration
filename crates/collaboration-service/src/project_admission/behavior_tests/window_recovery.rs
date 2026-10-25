use super::*;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn restore_clears_interrupted_selection_marks_and_keeps_draining_window_deadline()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let reader = fixture.reader.clone();
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadJoin {
                thread_id: fixture.root.clone(),
                role: ParticipantRole::Orchestrator,
                watch: true,
                mode: None,
                when_idle: None,
                replace: None,
                note: None,
            },
            fixture.now,
        )
        .await?;
    fixture
        .post_at(
            human("activity-author")?,
            "pending before restart",
            fixture.now,
        )
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadResolve {
                thread_id: fixture.root.clone(),
                reason: ThreadResolutionReason::try_from("done".to_owned())?,
            },
            fixture.now,
        )
        .await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let before = fixture
        .subscription(&reader, &scope, fixture.now)
        .await?
        .ok_or("subscription")?;
    assert_eq!(before.state(), SubscriptionState::Draining);
    assert_eq!(before.roots().len(), 1);
    let (_, selection) = fixture
        .windows
        .select_subscription_notice(
            &reader,
            std::slice::from_ref(&fixture.root),
            fixture.now + Duration::seconds(120),
            usize::MAX,
        )
        .await?;
    assert_eq!(selection.roots.len(), 1);
    let restored = fixture
        .windows
        .restore_active_and_draining(fixture.now + Duration::seconds(122))
        .await?
        .records
        .into_iter()
        .map(|row| row.record)
        .collect::<Vec<_>>();
    assert_eq!(restored.len(), 1);
    assert_eq!(
        restored.first().ok_or("subscription")?.state(),
        SubscriptionState::Draining
    );
    assert_eq!(
        restored
            .first()
            .ok_or("subscription")?
            .roots()
            .first()
            .ok_or("root")?
            .opened_at(),
        before.roots().first().ok_or("root")?.opened_at()
    );
    let progress = fixture.progress(&reader, &scope).await?.ok_or("progress")?;
    let window = progress
        .roots()
        .first()
        .ok_or("root")?
        .window
        .as_ref()
        .ok_or("window")?;
    assert!(window.selected.is_none());
    assert_eq!(
        window.opened_at,
        before.roots().first().ok_or("root")?.opened_at()
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn restart_before_notice_settlement_reselects_the_same_range() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    fixture
        .post_at(
            human("activity-author")?,
            "range survives selection",
            fixture.now,
        )
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    let at = fixture.now + Duration::seconds(120);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    let (before, _) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    assert_eq!(before.roots.len(), 1);
    assert!(
        fixture
            .windows
            .due_subscription_roots(&reader, at + Duration::seconds(1))
            .await?
            .is_empty()
    );
    // Reopen actual Router-local storage and reconstruct the existing window owner.
    fixture = fixture.reopen_progress().await?;
    fixture
        .windows
        .restore_active_and_draining(at + Duration::seconds(1))
        .await?;
    let roots = fixture
        .windows
        .due_subscription_roots(&reader, at + Duration::seconds(1))
        .await?;
    assert_eq!(roots, vec![fixture.root.clone()]);
    let (after, _) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at + Duration::seconds(1), usize::MAX)
        .await?;
    assert_eq!(after.roots, before.roots);
    fixture.stop().await
}
