use super::*;

fn join(thread: &ThreadId, role: ParticipantRole, watch: bool) -> BoardOperation {
    BoardOperation::ThreadJoin {
        thread_id: thread.clone(),
        role,
        watch,
        mode: None,
        when_idle: None,
        replace: None,
        note: None,
    }
}
fn poll_patch() -> SubscriptionPolicyPatch {
    SubscriptionPolicyPatch {
        mode: Some(SubscriptionMode::Poll),
        ..SubscriptionPolicyPatch::default()
    }
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn join_watch_creates_default_subscription_and_no_watch_does_not() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    fixture
        .admit(
            fixture.reader.clone(),
            join(&fixture.root, ParticipantRole::Participant, true),
            fixture.now,
        )
        .await?;
    let record = fixture
        .subscription(&fixture.reader, &scope, fixture.now)
        .await?
        .ok_or("membership missing")?;
    assert_eq!(record.state(), SubscriptionState::Active);
    assert_eq!(
        record.policy(),
        &SubscriptionPolicy::defaults_for(&fixture.reader)
    );
    let opted_out = session("no-watch-reader")?;
    fixture
        .admit(
            opted_out.clone(),
            join(&fixture.root, ParticipantRole::Participant, false),
            fixture.now,
        )
        .await?;
    assert!(
        fixture
            .subscription(&opted_out, &scope, fixture.now)
            .await?
            .is_none()
    );
    let store = fixture.project_store().await?;
    let mut read = store.snapshot().await?;
    assert!(
        read.effective_thread_watch(&fixture.root, &fixture.reader)
            .await?
            .is_some()
    );
    assert!(
        read.effective_thread_watch(&fixture.root, &opted_out)
            .await?
            .is_none()
    );
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn create_with_role_subscribes_the_session_creator() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let creator = session("creator")?;
    let post = PostId::generate();
    let root = ThreadId::from(post.clone());
    fixture
        .admit(
            creator.clone(),
            BoardOperation::ThreadCreate {
                post_id: post,
                about: fixture.discussion_subject.clone(),
                text: PostText::try_from("Created with role".to_owned())?,
                references: Vec::new(),
                participation: Some(ParticipantRole::Implementer),
                watch: true,
                addressees: Vec::new(),
            },
            fixture.now,
        )
        .await?;
    fixture.rescan(&creator, fixture.now).await?;
    let record = fixture
        .subscription(
            &creator,
            &SubscriptionScope::thread(root.clone()),
            fixture.now,
        )
        .await?
        .ok_or("creator membership missing")?;
    assert_eq!(record.state(), SubscriptionState::Active);
    assert_eq!(record.policy(), &SubscriptionPolicy::defaults_for(&creator));
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert!(
        read.effective_thread_watch(&root, &creator)
            .await?
            .is_some()
    );
    assert!(
        read.participants(&root)
            .await?
            .iter()
            .any(|participant| participant.identity == creator
                && participant.role
                    == RecordedParticipantRole::Recorded(ParticipantRole::Implementer)
                && participant.is_open())
    );
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn rejoin_reactivates_ended_subscription_and_keeps_policy() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let patch = SubscriptionPolicyPatch {
        mode: Some(SubscriptionMode::Poll),
        timing: SubscriptionTimingPatch {
            quiet_seconds: Some(60),
            cap_seconds: Some(600),
        },
        lifetime_seconds: Some(3600),
        ..SubscriptionPolicyPatch::default()
    };
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: scope.clone(),
                policy: patch,
            },
            fixture.now,
        )
        .await?;
    let patched = fixture
        .subscription(&fixture.reader, &scope, fixture.now)
        .await?
        .ok_or("patched missing")?;
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadUnsubscribe {
                scope: scope.clone(),
            },
            fixture.now + Duration::seconds(10),
        )
        .await?;
    let ended = fixture
        .subscription(&fixture.reader, &scope, fixture.now + Duration::seconds(10))
        .await?
        .ok_or("ended missing")?;
    assert_eq!(
        ended.state(),
        SubscriptionState::Ended {
            reason: EndReason::Cancelled
        }
    );
    let rejoined_at = fixture.now + Duration::seconds(20);
    fixture
        .admit(
            fixture.reader.clone(),
            join(&fixture.root, ParticipantRole::Participant, true),
            rejoined_at,
        )
        .await?;
    let reactivated = fixture
        .subscription(&fixture.reader, &scope, rejoined_at)
        .await?
        .ok_or("reactivated missing")?;
    assert_eq!(reactivated.state(), SubscriptionState::Active);
    assert_eq!(reactivated.policy(), patched.policy());
    assert_eq!(reactivated.renewed_at(), rejoined_at);
    assert!(reactivated.generation().get() > ended.generation().get());
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn thread_subscribe_checks_participant_validates_patch_and_activates_watch()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let reply = fixture
        .submit(
            fixture.reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: scope.clone(),
                policy: SubscriptionPolicyPatch::default(),
            },
            fixture.now,
        )
        .await?;
    assert!(refusals(&reply)?.iter().any(|reason| matches!(
        reason,
        Refusal::Thread(reason) if matches!(reason.as_ref(), ThreadRefusal::ParticipantRequired { .. })
    )));
    fixture
        .admit(
            fixture.reader.clone(),
            join(&fixture.root, ParticipantRole::Participant, false),
            fixture.now,
        )
        .await?;
    let patch = SubscriptionPolicyPatch {
        mode: Some(SubscriptionMode::Poll),
        timing: SubscriptionTimingPatch {
            quiet_seconds: Some(60),
            cap_seconds: None,
        },
        lifetime_seconds: Some(3600),
        ..SubscriptionPolicyPatch::default()
    };
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: scope.clone(),
                policy: patch,
            },
            fixture.now,
        )
        .await?;
    let record = fixture
        .subscription(&fixture.reader, &scope, fixture.now)
        .await?
        .ok_or("record missing")?;
    assert_eq!(record.policy().mode(), SubscriptionMode::Poll);
    assert_eq!(record.policy().timing().quiet_seconds(), 60);
    assert_eq!(record.policy().timing().cap_seconds(), 600);
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert!(
        read.effective_thread_watch(&fixture.root, &fixture.reader)
            .await?
            .is_some()
    );
    read.finish().await?;
    let invalid = SubscriptionPolicyPatch {
        timing: SubscriptionTimingPatch {
            quiet_seconds: Some(300),
            cap_seconds: Some(120),
        },
        ..SubscriptionPolicyPatch::default()
    };
    let reply = fixture
        .submit(
            fixture.reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: scope.clone(),
                policy: invalid,
            },
            fixture.now + Duration::seconds(30),
        )
        .await?;
    assert!(
        refusals(&reply)?
            .iter()
            .any(|reason| matches!(reason, Refusal::InvalidShape { .. }))
    );
    assert_eq!(
        fixture
            .subscription(&fixture.reader, &scope, fixture.now)
            .await?,
        Some(record)
    );
    let person = human("non-push-reader")?;
    let project_scope = SubscriptionScope::project(fixture.project.clone());
    let reply = fixture
        .submit(
            person.clone(),
            BoardOperation::ThreadSubscribe {
                scope: project_scope.clone(),
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Deliver),
                    ..SubscriptionPolicyPatch::default()
                },
            },
            fixture.now,
        )
        .await?;
    assert!(
        refusals(&reply)?
            .iter()
            .any(|reason| matches!(reason, Refusal::InvalidShape { .. }))
    );
    assert!(
        fixture
            .subscription(&person, &project_scope, fixture.now)
            .await?
            .is_none()
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn unsubscribe_keeps_watch_but_unwatch_ends_without_post_reactivation()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    fixture
        .post_at(human("other")?, "pending before unsubscribe", fixture.now)
        .await?;
    fixture.rescan(&fixture.reader.clone(), fixture.now).await?;
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadUnsubscribe {
                scope: scope.clone(),
            },
            fixture.now,
        )
        .await?;
    let cancelled = fixture
        .subscription(&fixture.reader, &scope, fixture.now)
        .await?
        .ok_or("cancelled missing")?;
    assert_eq!(
        cancelled.state(),
        SubscriptionState::Ended {
            reason: EndReason::Cancelled
        }
    );
    assert!(cancelled.roots().is_empty());
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert!(
        read.effective_thread_watch(&fixture.root, &fixture.reader)
            .await?
            .is_some()
    );
    read.finish().await?;
    fixture
        .post_at(
            human("other")?,
            "does not recreate subscription",
            fixture.now,
        )
        .await?;
    fixture.rescan(&fixture.reader.clone(), fixture.now).await?;
    assert_eq!(
        fixture
            .subscription(&fixture.reader, &scope, fixture.now)
            .await?
            .ok_or("record missing")?
            .state(),
        cancelled.state()
    );
    for (label, rejoin) in [("unwatch-reader", false), ("active-then-no-watch", true)] {
        let reader = session(label)?;
        fixture
            .admit(
                reader.clone(),
                join(&fixture.root, ParticipantRole::Participant, true),
                fixture.now,
            )
            .await?;
        fixture.rescan(&reader, fixture.now).await?;
        fixture
            .post_at(
                human("window-opener")?,
                "creates a window",
                fixture.now + Duration::seconds(1),
            )
            .await?;
        fixture
            .rescan(&reader, fixture.now + Duration::seconds(1))
            .await?;
        let operation = if rejoin {
            join(&fixture.root, ParticipantRole::Participant, false)
        } else {
            BoardOperation::ThreadUnwatch {
                thread_id: fixture.root.clone(),
            }
        };
        fixture
            .admit(
                reader.clone(),
                operation,
                fixture.now + Duration::seconds(2),
            )
            .await?;
        fixture
            .rescan(&reader, fixture.now + Duration::seconds(2))
            .await?;
        let record = fixture
            .subscription(&reader, &scope, fixture.now + Duration::seconds(2))
            .await?
            .ok_or("ended missing")?;
        assert_eq!(
            record.state(),
            SubscriptionState::Ended {
                reason: EndReason::Cancelled
            }
        );
        assert!(record.roots().is_empty());
        let mut read = fixture.project_store().await?.snapshot().await?;
        assert!(
            read.effective_thread_watch(&fixture.root, &reader)
                .await?
                .is_none()
        );
        read.finish().await?;
        fixture
            .post_at(
                human("later-author")?,
                "must not recreate ended subscription",
                fixture.now + Duration::seconds(3),
            )
            .await?;
        fixture
            .rescan(&reader, fixture.now + Duration::seconds(3))
            .await?;
        assert_eq!(
            fixture
                .subscription(&reader, &scope, fixture.now + Duration::seconds(3))
                .await?
                .ok_or("record missing")?
                .state(),
            record.state()
        );
    }
    let project_reader = session("project-unwatch-reader")?;
    let project_scope = SubscriptionScope::project(fixture.project.clone());
    fixture
        .admit(
            project_reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: project_scope.clone(),
                policy: poll_patch(),
            },
            fixture.now,
        )
        .await?;
    fixture.rescan(&project_reader, fixture.now).await?;
    fixture
        .post_at(
            human("project-activity-author")?,
            "window before project unwatch",
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .rescan(&project_reader, fixture.now + Duration::seconds(1))
        .await?;
    assert_eq!(
        fixture
            .subscription(
                &project_reader,
                &project_scope,
                fixture.now + Duration::seconds(1)
            )
            .await?
            .ok_or("project subscription missing")?
            .roots()
            .len(),
        1
    );
    fixture
        .admit(
            project_reader.clone(),
            BoardOperation::ProjectUnwatch {},
            fixture.now + Duration::seconds(2),
        )
        .await?;
    fixture
        .rescan(&project_reader, fixture.now + Duration::seconds(2))
        .await?;
    let record = fixture
        .subscription(
            &project_reader,
            &project_scope,
            fixture.now + Duration::seconds(2),
        )
        .await?
        .ok_or("project record missing")?;
    assert_eq!(
        record.state(),
        SubscriptionState::Ended {
            reason: EndReason::Cancelled
        }
    );
    assert!(record.roots().is_empty());
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert!(
        read.effective_thread_watch(&fixture.root, &project_reader)
            .await?
            .is_some()
    );
    read.finish().await?;
    fixture
        .post_at(
            human("after-project-unwatch")?,
            "kept in the watched thread inbox",
            fixture.now + Duration::seconds(3),
        )
        .await?;
    fixture
        .rescan(&project_reader, fixture.now + Duration::seconds(200))
        .await?;
    assert!(
        fixture
            .windows
            .due_subscription_roots(&project_reader, fixture.now + Duration::seconds(200))
            .await?
            .is_empty()
    );
    fixture.stop().await
}
