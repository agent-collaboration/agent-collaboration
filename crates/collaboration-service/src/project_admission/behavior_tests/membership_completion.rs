//! Retained completion/expiry claims keep replicated end facts and local windows separate.
use super::*;

pub(super) fn join_operation(thread: &ThreadId, role: ParticipantRole) -> BoardOperation {
    BoardOperation::ThreadJoin {
        thread_id: thread.clone(),
        role,
        watch: true,
        mode: None,
        when_idle: None,
        replace: None,
        note: None,
    }
}
fn resolve_operation(thread: &ThreadId) -> Result<BoardOperation, TestError> {
    Ok(BoardOperation::ThreadResolve {
        thread_id: thread.clone(),
        reason: ThreadResolutionReason::try_from("done".to_owned())?,
    })
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn leave_replacement_and_expiry_end_rows_and_delete_windows() -> Result<(), TestError> {
    let mut left = SubscriptionFixture::joined().await?;
    let scope = SubscriptionScope::thread(left.root.clone());
    left.post_at(human("writer")?, "pending before leave", left.now)
        .await?;
    left.rescan(&left.reader.clone(), left.now).await?;
    left.admit(
        left.reader.clone(),
        BoardOperation::ThreadLeave {
            thread_id: left.root.clone(),
            after_leave: None,
        },
        left.now,
    )
    .await?;
    let record = left
        .subscription(&left.reader, &scope, left.now)
        .await?
        .ok_or("left membership missing")?;
    assert_eq!(
        record.state(),
        SubscriptionState::Ended {
            reason: EndReason::Left
        }
    );
    assert!(record.roots().is_empty());
    left.stop().await?;

    let mut replaced = SubscriptionFixture::start().await?;
    let scope = SubscriptionScope::thread(replaced.root.clone());
    replaced
        .admit(
            replaced.reader.clone(),
            join_operation(&replaced.root, ParticipantRole::Implementer),
            replaced.now,
        )
        .await?;
    replaced
        .post_at(human("writer")?, "pending before replacement", replaced.now)
        .await?;
    replaced
        .rescan(&replaced.reader.clone(), replaced.now)
        .await?;
    assert_eq!(
        replaced
            .subscription(&replaced.reader, &scope, replaced.now)
            .await?
            .ok_or("record missing")?
            .roots()
            .len(),
        1
    );
    let mut operation = join_operation(&replaced.root, ParticipantRole::Implementer);
    if let BoardOperation::ThreadJoin { replace, .. } = &mut operation {
        *replace = Some(replaced.reader.clone());
    }
    replaced
        .admit(
            session("replacement")?,
            operation,
            replaced.now + Duration::seconds(1),
        )
        .await?;
    let record = replaced
        .subscription(
            &replaced.reader,
            &scope,
            replaced.now + Duration::seconds(1),
        )
        .await?
        .ok_or("replaced missing")?;
    assert_eq!(
        record.state(),
        SubscriptionState::Ended {
            reason: EndReason::Replaced
        }
    );
    assert!(record.roots().is_empty());
    replaced.stop().await?;

    let mut expired = SubscriptionFixture::joined().await?;
    let scope = SubscriptionScope::thread(expired.root.clone());
    expired
        .post_at(human("writer")?, "pending before expiry", expired.now)
        .await?;
    expired.rescan(&expired.reader.clone(), expired.now).await?;
    let read = expired.project_store().await?.snapshot().await?;
    let before = read.high_water();
    read.finish().await?;
    let at = expired.now + Duration::hours(25);
    expired.rescan(&expired.reader.clone(), at).await?;
    let record = expired
        .subscription(&expired.reader, &scope, at)
        .await?
        .ok_or("expired missing")?;
    assert_eq!(
        record.state(),
        SubscriptionState::Ended {
            reason: EndReason::Expired
        }
    );
    assert!(record.roots().is_empty());
    assert!(
        expired
            .windows
            .due_subscription_roots(&expired.reader, at)
            .await?
            .is_empty()
    );
    let mut read = expired.project_store().await?.snapshot().await?;
    assert_eq!(
        read.high_water(),
        before,
        "deriving expiry must not allocate replicated activity"
    );
    let memberships = read
        .subscription_memberships(Some(&expired.reader))
        .await?
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    assert!(
        memberships
            .iter()
            .all(|membership| membership.ended().is_none())
    );
    read.finish().await?;
    expired.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn resolve_drains_pending_or_ends_mode_off_subscription() -> Result<(), TestError> {
    for mode_off in [false, true] {
        let mut fixture = SubscriptionFixture::joined().await?;
        let scope = SubscriptionScope::thread(fixture.root.clone());
        if mode_off {
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
                    fixture.now,
                )
                .await?;
        }
        fixture
            .post_at(human("writer")?, "pending when resolved", fixture.now)
            .await?;
        fixture.rescan(&fixture.reader.clone(), fixture.now).await?;
        fixture
            .admit(
                human("owner")?,
                resolve_operation(&fixture.root)?,
                fixture.now + Duration::seconds(1),
            )
            .await?;
        let record = fixture
            .subscription(&fixture.reader, &scope, fixture.now + Duration::seconds(1))
            .await?
            .ok_or("resolved membership missing")?;
        if mode_off {
            assert_eq!(
                record.state(),
                SubscriptionState::Ended {
                    reason: EndReason::Resolved
                }
            );
            assert!(record.roots().is_empty());
        } else {
            assert_eq!(record.state(), SubscriptionState::Draining);
            assert_eq!(record.roots().len(), 1);
            assert_eq!(
                fixture
                    .windows
                    .due_subscription_roots(&fixture.reader, fixture.now + Duration::seconds(120))
                    .await?,
                vec![fixture.root.clone()]
            );
        }
        let mut read = fixture.project_store().await?.snapshot().await?;
        assert!(
            read.effective_thread_watch(&fixture.root, &fixture.reader)
                .await?
                .is_some()
        );
        read.finish().await?;
        fixture.stop().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn leave_resolve_ends_resolver_as_left_and_drains_other_readers() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let resolver = fixture.reader.clone();
    let other = session("other-reader")?;
    fixture
        .admit(
            resolver.clone(),
            join_operation(&fixture.root, ParticipantRole::Orchestrator),
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            other.clone(),
            join_operation(&fixture.root, ParticipantRole::Participant),
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .rescan(&other, fixture.now + Duration::seconds(1))
        .await?;
    fixture
        .post_at(
            human("writer")?,
            "pending before resolver leaves",
            fixture.now + Duration::seconds(2),
        )
        .await?;
    fixture
        .rescan(&resolver, fixture.now + Duration::seconds(2))
        .await?;
    fixture
        .rescan(&other, fixture.now + Duration::seconds(2))
        .await?;
    fixture
        .admit(
            resolver.clone(),
            BoardOperation::ThreadLeave {
                thread_id: fixture.root.clone(),
                after_leave: Some(LeaveFollowUp::Resolve {
                    reason: ThreadResolutionReason::try_from("done".to_owned())?,
                }),
            },
            fixture.now + Duration::seconds(3),
        )
        .await?;
    fixture
        .rescan(&other, fixture.now + Duration::seconds(3))
        .await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let left = fixture
        .subscription(&resolver, &scope, fixture.now + Duration::seconds(3))
        .await?
        .ok_or("left missing")?;
    assert_eq!(
        left.state(),
        SubscriptionState::Ended {
            reason: EndReason::Left
        }
    );
    assert!(left.roots().is_empty());
    let draining = fixture
        .subscription(&other, &scope, fixture.now + Duration::seconds(3))
        .await?
        .ok_or("other missing")?;
    assert_eq!(draining.state(), SubscriptionState::Draining);
    assert_eq!(draining.roots().len(), 1);
    assert_eq!(
        draining
            .roots()
            .first()
            .ok_or("window missing")?
            .pending_count(),
        1
    );
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert!(
        read.effective_thread_watch(&fixture.root, &resolver)
            .await?
            .is_none()
    );
    assert!(
        read.effective_thread_watch(&fixture.root, &other)
            .await?
            .is_some()
    );
    read.finish().await?;
    assert_eq!(
        fixture
            .windows
            .due_subscription_roots(&other, fixture.now + Duration::seconds(130))
            .await?,
        vec![fixture.root.clone()]
    );
    fixture.stop().await
}
