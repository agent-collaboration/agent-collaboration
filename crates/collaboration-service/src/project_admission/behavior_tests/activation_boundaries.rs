//! Activation is a ledger position; policy updates preserve backlog while old receipts are fenced.
use super::*;

async fn membership(fixture: &SubscriptionFixture) -> Result<SubscriptionMembership, TestError> {
    let mut read = fixture.project_store().await?.snapshot().await?;
    let record = read
        .subscription_memberships(Some(&fixture.reader))
        .await?
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|member| member.scope() == &SubscriptionScope::thread(fixture.root.clone()))
        .ok_or("membership missing")?;
    read.finish().await?;
    Ok(record)
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn activation_is_the_request_position_and_reactivation_changes_the_local_key()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let first = membership(&fixture).await?;
    let request = first
        .envelope
        .request
        .clone()
        .ok_or("activation request missing")?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    let ledger = read
        .record(&request.into())
        .await?
        .ok_or("ledger missing")?;
    assert_eq!(
        RecordPosition::Project(first.activated_at()),
        ledger.envelope().position
    );
    read.finish().await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    fixture
        .post_at(
            human("writer")?,
            "old pending activity",
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .rescan(&fixture.reader.clone(), fixture.now + Duration::seconds(1))
        .await?;
    let old_progress = fixture
        .progress(&fixture.reader, &scope)
        .await?
        .ok_or("progress missing")?;
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadUnsubscribe {
                scope: scope.clone(),
            },
            fixture.now + Duration::seconds(2),
        )
        .await?;
    fixture
        .post_at(
            human("writer")?,
            "while unsubscribed",
            fixture.now + Duration::seconds(3),
        )
        .await?;
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadJoin {
                thread_id: fixture.root.clone(),
                role: ParticipantRole::Participant,
                watch: true,
                mode: None,
                when_idle: None,
                replace: None,
                note: None,
            },
            fixture.now + Duration::seconds(4),
        )
        .await?;
    let current = membership(&fixture).await?;
    assert_ne!(current.activated_at(), first.activated_at());
    assert!(current.generation().get() > first.generation().get());
    let current_progress = fixture
        .progress(&fixture.reader, &scope)
        .await?
        .ok_or("new progress missing")?;
    assert_eq!(current_progress.activated_at(), current.activated_at());
    assert!(current_progress.roots().is_empty());
    assert_eq!(
        fixture
            .automation
            .lock()
            .await
            .subscription_progress(&fixture.project, &first.envelope.id, first.activated_at())
            .await?
            .ok_or("old activation missing")?
            .scan_through(),
        old_progress.scan_through()
    );
    assert!(
        fixture
            .windows
            .due_subscription_roots(&fixture.reader, fixture.now + Duration::seconds(200))
            .await?
            .is_empty()
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn policy_change_preserves_backlog_releases_old_selection_and_fences_late_settlement()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    fixture
        .post_at(
            human("writer")?,
            "selected under old policy",
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .rescan(&fixture.reader.clone(), fixture.now + Duration::seconds(1))
        .await?;
    let at = fixture.now + Duration::seconds(121);
    let (_, old) = fixture
        .windows
        .select_subscription_notice(&fixture.reader, &[fixture.root.clone()], at, usize::MAX)
        .await?;
    assert_eq!(old.roots.len(), 1);
    let before = membership(&fixture).await?;
    let progress = fixture
        .progress(&fixture.reader, &scope)
        .await?
        .ok_or("old progress missing")?;
    let request = ProjectRequest {
        request_id: RequestId::generate(),
        project_id: fixture.project.clone(),
        actor: fixture.reader.clone(),
        acting_for: None,
        observed_epoch: Some(EpochNumber::try_from(1)?),
        resubmits: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: BoardOperation::ThreadSubscribe {
            scope: scope.clone(),
            policy: SubscriptionPolicyPatch {
                mode: Some(SubscriptionMode::Poll),
                timing: SubscriptionTimingPatch {
                    quiet_seconds: Some(0),
                    cap_seconds: Some(0),
                },
                ..SubscriptionPolicyPatch::default()
            },
        },
    };
    let accepted = fixture
        .admission
        .submit_at(&request, at + Duration::seconds(1))
        .await?;
    assert_accepted(&accepted);
    fixture
        .rescan(&fixture.reader.clone(), at + Duration::seconds(1))
        .await?;
    let after = membership(&fixture).await?;
    assert_eq!(after.activated_at(), before.activated_at());
    assert!(after.generation().get() > before.generation().get());
    let replay = fixture
        .admission
        .submit_at(&request, at + Duration::seconds(2))
        .await?;
    assert_eq!(replay.result, accepted.result);
    assert_eq!(
        membership(&fixture).await?.activated_at(),
        after.activated_at()
    );
    let current = fixture
        .progress(&fixture.reader, &scope)
        .await?
        .ok_or("progress missing")?;
    let root = current.roots().first().ok_or("pending root missing")?;
    assert_eq!(
        root.delivered_through,
        progress
            .roots()
            .first()
            .ok_or("old root missing")?
            .delivered_through
    );
    assert!(
        root.window
            .as_ref()
            .ok_or("window missing")?
            .selected
            .is_none()
    );
    assert_eq!(root.window.as_ref().ok_or("window missing")?.post_count, 1);
    let due = fixture
        .windows
        .due_subscription_roots(&fixture.reader, at + Duration::seconds(2))
        .await?;
    assert_eq!(due, vec![fixture.root.clone()]);
    let (_, fresh) = fixture
        .windows
        .select_subscription_notice(&fixture.reader, &due, at + Duration::seconds(2), usize::MAX)
        .await?;
    fixture
        .windows
        .settle_subscription_batch(&fixture.reader, &old, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    let still = fixture
        .progress(&fixture.reader, &scope)
        .await?
        .ok_or("progress missing")?;
    assert!(
        still
            .roots()
            .first()
            .and_then(|root| root.window.as_ref())
            .is_some_and(|window| window.selected.is_some())
    );
    assert_eq!(still.last_outcome(), None);
    assert_eq!(
        still
            .roots()
            .first()
            .ok_or("root missing")?
            .delivered_through,
        root.delivered_through
    );
    fixture
        .windows
        .settle_subscription_batch(
            &fixture.reader,
            &fresh,
            SubscriptionDeliveryOutcome::Accepted,
        )
        .await?;
    assert!(
        fixture
            .progress(&fixture.reader, &scope)
            .await?
            .ok_or("progress missing")?
            .roots()
            .iter()
            .all(|root| root.window.is_none())
    );
    fixture.stop().await
}
