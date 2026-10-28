use super::membership_completion::join_operation;
use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn project_subscription_keeps_off_activity_unread_and_thread_rows_take_precedence()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let scope = SubscriptionScope::project(fixture.project.clone());
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
    let post = fixture
        .post_at(
            human("writer")?,
            "off mode remains unread",
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .rescan(&fixture.reader.clone(), fixture.now + Duration::seconds(1))
        .await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert!(
        read.effective_thread_watch(&fixture.root, &fixture.reader)
            .await?
            .is_some()
    );
    read.finish().await?;
    assert!(
        fixture
            .unread_posts(fixture.reader.clone())
            .await?
            .contains(&post.post_id)
    );
    assert!(
        fixture
            .subscription(&fixture.reader, &scope, fixture.now + Duration::seconds(1))
            .await?
            .ok_or("project membership missing")?
            .roots()
            .is_empty()
    );
    let new_root_post = PostId::generate();
    let new_root = ThreadId::from(new_root_post.clone());
    fixture
        .admit(
            human("new-root-creator")?,
            BoardOperation::ThreadCreate {
                post_id: new_root_post.clone(),
                about: fixture.discussion_subject.clone(),
                text: PostText::try_from("root after project subscribe".to_owned())?,
                references: Vec::new(),
                participation: None,
                watch: false,
                addressees: Vec::new(),
            },
            fixture.now + Duration::seconds(2),
        )
        .await?;
    let new_reply = PostId::generate();
    fixture
        .admit(
            human("new-root-writer")?,
            BoardOperation::ThreadPost {
                post_id: new_reply.clone(),
                thread_id: new_root.clone(),
                text: PostText::try_from("new root reply".to_owned())?,
                references: Vec::new(),
                addressees: Vec::new(),
            },
            fixture.now + Duration::seconds(3),
        )
        .await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert!(
        read.effective_thread_watch(&new_root, &fixture.reader)
            .await?
            .is_some()
    );
    read.finish().await?;
    assert!(
        fixture
            .unread_posts(fixture.reader.clone())
            .await?
            .contains(&new_reply)
    );
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadSubscribe {
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
            fixture.now + Duration::seconds(4),
        )
        .await?;
    let due = fixture
        .windows
        .due_subscription_roots(&fixture.reader, fixture.now + Duration::seconds(125))
        .await?;
    assert_eq!(
        due.len(),
        2,
        "rescan must recover both previously off roots"
    );
    assert!(due.contains(&fixture.root));
    assert!(due.contains(&new_root));
    let (_, settlement) = fixture
        .windows
        .select_subscription_notice(
            &fixture.reader,
            &due,
            fixture.now + Duration::seconds(125),
            usize::MAX,
        )
        .await?;
    fixture
        .windows
        .settle_subscription_batch(
            &fixture.reader,
            &settlement,
            SubscriptionDeliveryOutcome::Accepted,
        )
        .await?;
    assert!(
        fixture
            .unread_posts(fixture.reader.clone())
            .await?
            .contains(&post.post_id)
    );
    assert!(
        fixture
            .unread_posts(fixture.reader.clone())
            .await?
            .contains(&new_reply)
    );
    fixture
        .admit(
            fixture.reader.clone(),
            join_operation(&fixture.root, ParticipantRole::Participant),
            fixture.now + Duration::seconds(126),
        )
        .await?;
    let thread_scope = SubscriptionScope::thread(fixture.root.clone());
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: thread_scope.clone(),
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Off),
                    ..SubscriptionPolicyPatch::default()
                },
            },
            fixture.now + Duration::seconds(127),
        )
        .await?;
    fixture
        .post_at(
            human("writer")?,
            "active thread row shadows project",
            fixture.now + Duration::seconds(128),
        )
        .await?;
    fixture
        .rescan(
            &fixture.reader.clone(),
            fixture.now + Duration::seconds(128),
        )
        .await?;
    assert!(
        fixture
            .subscription(
                &fixture.reader,
                &thread_scope,
                fixture.now + Duration::seconds(128)
            )
            .await?
            .ok_or("thread missing")?
            .roots()
            .is_empty()
    );
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadUnsubscribe {
                scope: thread_scope,
            },
            fixture.now + Duration::seconds(129),
        )
        .await?;
    fixture
        .post_at(
            human("writer")?,
            "ended thread row still shadows project",
            fixture.now + Duration::seconds(130),
        )
        .await?;
    fixture
        .rescan(
            &fixture.reader.clone(),
            fixture.now + Duration::seconds(130),
        )
        .await?;
    assert!(
        fixture
            .subscription(
                &fixture.reader,
                &scope,
                fixture.now + Duration::seconds(130)
            )
            .await?
            .ok_or("project missing")?
            .roots()
            .is_empty()
    );
    fixture.stop().await
}
