//! Subscription activation and later posts retain their distinct thread-watch effects.
use super::*;

fn project_subscribe(project: &ProjectId) -> BoardOperation {
    BoardOperation::ThreadSubscribe {
        scope: SubscriptionScope::project(project.clone()),
        policy: SubscriptionPolicyPatch {
            mode: Some(SubscriptionMode::Poll),
            timing: SubscriptionTimingPatch {
                quiet_seconds: Some(0),
                cap_seconds: Some(0),
            },
            ..SubscriptionPolicyPatch::default()
        },
    }
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn subscription_activation_creates_existing_watches_and_post_replay_keeps_their_boundary()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    fixture
        .admit(
            fixture.reader.clone(),
            project_subscribe(&fixture.project),
            fixture.now,
        )
        .await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    let activation_boundary = read
        .thread_watch(&fixture.root, &fixture.reader)
        .await?
        .ok_or("subscription activation did not materialize the existing thread")?;
    read.finish().await?;
    let request = ProjectRequest {
        request_id: RequestId::generate(),
        project_id: fixture.project.clone(),
        actor: human("writer")?,
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: BoardOperation::ThreadPost {
            post_id: PostId::generate(),
            thread_id: fixture.root.clone(),
            text: PostText::try_from("activity pulls this reader in".to_owned())?,
            references: Vec::new(),
            addressees: Vec::new(),
        },
    };
    let accepted = fixture
        .admission
        .submit_at(&request, fixture.now + Duration::seconds(1))
        .await?;
    assert_accepted(&accepted);
    let mut read = fixture.project_store().await?.snapshot().await?;
    let head = read.high_water();
    let boundary = read
        .thread_watch(&fixture.root, &fixture.reader)
        .await?
        .ok_or("activity did not materialize watch")?;
    assert_eq!(
        boundary, activation_boundary,
        "a later post preserves an active watch start"
    );
    read.finish().await?;
    let replay = fixture
        .admission
        .submit_at(&request, fixture.now + Duration::seconds(2))
        .await?;
    assert_eq!(replay.result, accepted.result);
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert_eq!(read.high_water(), head);
    assert_eq!(
        read.thread_watch(&fixture.root, &fixture.reader).await?,
        Some(boundary)
    );
    read.finish().await?;
    fixture
        .rescan(&fixture.reader.clone(), fixture.now + Duration::seconds(2))
        .await?;
    assert_eq!(
        fixture
            .windows
            .due_subscription_roots(&fixture.reader, fixture.now + Duration::seconds(2))
            .await?,
        vec![fixture.root.clone()]
    );
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ProjectUnwatch {},
            fixture.now + Duration::seconds(3),
        )
        .await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert!(read.project_watch(&fixture.reader).await?.is_none());
    assert_eq!(
        read.effective_thread_watch(&fixture.root, &fixture.reader)
            .await?,
        Some(boundary)
    );
    read.finish().await?;
    assert!(
        fixture
            .windows
            .due_subscription_roots(&fixture.reader, fixture.now + Duration::seconds(3))
            .await?
            .is_empty(),
        "inbox recovery survives without a push"
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn p6_project_coverage_does_not_reactivate_an_explicitly_unwatched_thread()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    fixture
        .admit(
            fixture.reader.clone(),
            project_subscribe(&fixture.project),
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadUnwatch {
                thread_id: fixture.root.clone(),
            },
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .post_at(
            human("writer")?,
            "must not reactivate the watch",
            fixture.now + Duration::seconds(2),
        )
        .await?;
    fixture
        .rescan(&fixture.reader.clone(), fixture.now + Duration::seconds(2))
        .await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert!(
        read.effective_thread_watch(&fixture.root, &fixture.reader)
            .await?
            .is_none()
    );
    read.finish().await?;
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
async fn p6_join_without_watch_keeps_its_disabled_row_on_later_posts() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    fixture
        .admit(
            fixture.reader.clone(),
            project_subscribe(&fixture.project),
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadJoin {
                thread_id: fixture.root.clone(),
                role: ParticipantRole::Participant,
                watch: false,
                mode: None,
                when_idle: None,
                replace: None,
                note: None,
            },
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .post_at(
            human("writer")?,
            "must honor the join opt-out",
            fixture.now + Duration::seconds(2),
        )
        .await?;
    fixture
        .rescan(&fixture.reader.clone(), fixture.now + Duration::seconds(2))
        .await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert!(
        read.effective_thread_watch(&fixture.root, &fixture.reader)
            .await?
            .is_none()
    );
    read.finish().await?;
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
async fn q2_project_subscribe_reactivates_a_plain_opt_out_without_a_thread_subscription_override()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadWatch {
                thread_id: fixture.root.clone(),
            },
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadUnwatch {
                thread_id: fixture.root.clone(),
            },
            fixture.now,
        )
        .await?;
    assert!(
        fixture
            .subscription(
                &fixture.reader,
                &SubscriptionScope::thread(fixture.root.clone()),
                fixture.now
            )
            .await?
            .is_none()
    );
    fixture
        .admit(
            fixture.reader.clone(),
            project_subscribe(&fixture.project),
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .post_at(
            human("writer")?,
            "subscription activation restores the plain watch",
            fixture.now + Duration::seconds(2),
        )
        .await?;
    fixture
        .rescan(&fixture.reader.clone(), fixture.now + Duration::seconds(2))
        .await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert!(
        read.effective_thread_watch(&fixture.root, &fixture.reader)
            .await?
            .is_some()
    );
    read.finish().await?;
    assert_eq!(
        fixture
            .windows
            .due_subscription_roots(&fixture.reader, fixture.now + Duration::seconds(200))
            .await?,
        vec![fixture.root.clone()]
    );
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadWatch {
                thread_id: fixture.root.clone(),
            },
            fixture.now + Duration::seconds(3),
        )
        .await?;
    fixture
        .post_at(
            human("writer")?,
            "explicit root watch restores coverage",
            fixture.now + Duration::seconds(4),
        )
        .await?;
    fixture
        .rescan(&fixture.reader.clone(), fixture.now + Duration::seconds(4))
        .await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert!(
        read.effective_thread_watch(&fixture.root, &fixture.reader)
            .await?
            .is_some()
    );
    read.finish().await?;
    assert_eq!(
        fixture
            .windows
            .due_subscription_roots(&fixture.reader, fixture.now + Duration::seconds(200))
            .await?,
        vec![fixture.root.clone()]
    );
    fixture.stop().await
}
