use super::membership_completion::join_operation;
use super::*;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Session participation and conversation leadership are independent gates.
async fn session_thread_subscriptions_require_join_while_project_humans_remain_exempt()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let unjoined = session("unjoined")?;
    let reply = fixture
        .submit(
            unjoined.clone(),
            BoardOperation::ThreadPost {
                post_id: PostId::generate(),
                thread_id: fixture.root.clone(),
                text: PostText::try_from("blocked".to_owned())?,
                references: Vec::new(),
                addressees: Vec::new(),
            },
            fixture.now,
        )
        .await?;
    assert!(refusals(&reply)?.iter().any(|reason|matches!(reason,Refusal::Thread(reason) if matches!(reason.as_ref(),ThreadRefusal::ParticipantRequired {..}))));
    let resolve = BoardOperation::ThreadResolve {
        thread_id: fixture.root.clone(),
        reason: ThreadResolutionReason::try_from("done".to_owned())?,
    };
    let reply = fixture
        .submit(unjoined.clone(), resolve.clone(), fixture.now)
        .await?;
    assert!(refusals(&reply)?.iter().any(|reason|matches!(reason,Refusal::Thread(reason) if matches!(reason.as_ref(),ThreadRefusal::ParticipantRequired {..}))));
    let reviewer = session("joined-non-orchestrator")?;
    fixture
        .admit(
            reviewer.clone(),
            join_operation(&fixture.root, ParticipantRole::Reviewer),
            fixture.now,
        )
        .await?;
    let reply = fixture.submit(reviewer, resolve, fixture.now).await?;
    assert!(refusals(&reply)?.iter().any(|reason|matches!(reason,Refusal::Thread(reason) if matches!(reason.as_ref(),ThreadRefusal::OrchestratorRequired {..}))));
    let reply = fixture
        .submit(
            unjoined,
            BoardOperation::ThreadSubscribe {
                scope: SubscriptionScope::thread(fixture.root.clone()),
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Poll),
                    ..Default::default()
                },
            },
            fixture.now,
        )
        .await?;
    assert!(refusals(&reply)?.iter().any(|reason|matches!(reason,Refusal::Thread(reason) if matches!(reason.as_ref(),ThreadRefusal::ParticipantRequired {..}))));
    let reader = human("human-reader")?;
    fixture
        .post_at(reader.clone(), "human reply", fixture.now)
        .await?;
    let shown = fixture
        .application
        .board()
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (fixture.root.clone()).into(),
            reader: ReadActor::Identified {
                reader: reader.clone(),
            },
        })
        .await?;
    assert!(!shown.watch_status.ok_or("watch")?.watching);
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
    assert_eq!(
        fixture
            .subscription(&reader, &scope, fixture.now)
            .await?
            .ok_or("subscription")?
            .policy()
            .mode(),
        SubscriptionMode::Poll
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Each scope's refusal identifies that scope and creates no watch or membership.
async fn thread_subscriptions_refuse_each_missing_session_participant_without_creating_watches()
-> Result<(), TestError> {
    let fixture = SubscriptionFixture::start().await?;
    let second = ThreadId::generate();
    fixture
        .create_root(&second, human("owner-two")?, "second root")
        .await?;
    let reader = session("unjoined-listener")?;
    for root in [&fixture.root, &second] {
        let reply = fixture
            .submit(
                reader.clone(),
                BoardOperation::ThreadSubscribe {
                    scope: SubscriptionScope::thread(root.clone()),
                    policy: SubscriptionPolicyPatch {
                        mode: Some(SubscriptionMode::Poll),
                        ..Default::default()
                    },
                },
                fixture.now,
            )
            .await?;
        assert!(refusals(&reply)?.iter().any(|reason|matches!(reason,Refusal::Thread(reason) if matches!(reason.as_ref(),ThreadRefusal::ParticipantRequired {thread_id,actor} if thread_id==root && actor==&reader))));
        let mut read = fixture.project_store().await?.snapshot().await?;
        assert!(read.thread_watch(root, &reader).await?.is_none());
        assert!(
            read.membership(&reader, &SubscriptionScope::thread(root.clone()))
                .await?
                .is_none()
        );
        read.finish().await?;
    }
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Competing requests cross the actual per-project writer lock.
async fn concurrent_orchestrator_joins_commit_one_holder_and_report_the_winner()
-> Result<(), TestError> {
    let fixture = SubscriptionFixture::start().await?;
    let first = session("first-racer")?;
    let second = session("second-racer")?;
    let barrier = tokio::sync::Barrier::new(2);
    let racing_first = async {
        barrier.wait().await;
        fixture
            .submit(
                first.clone(),
                join_operation(&fixture.root, ParticipantRole::Orchestrator),
                fixture.now,
            )
            .await
    };
    let racing_second = async {
        barrier.wait().await;
        fixture
            .submit(
                second.clone(),
                join_operation(&fixture.root, ParticipantRole::Orchestrator),
                fixture.now,
            )
            .await
    };
    let (one, two) = tokio::join!(racing_first, racing_second);
    let one = one?;
    let two = two?;
    let accepted = |reply: &WriteReply| {
        matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        )
    };
    assert_eq!(usize::from(accepted(&one)) + usize::from(accepted(&two)), 1);
    let loser = if accepted(&one) { &two } else { &one };
    let winner = if accepted(&one) { first } else { second };
    assert!(refusals(loser)?.iter().any(|reason|matches!(reason,Refusal::Thread(reason) if matches!(reason.as_ref(),ThreadRefusal::OrchestratorAlreadyExists {holder,..} if holder==&winner))));
    let shown = fixture
        .application
        .board()
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (fixture.root.clone()).into(),
            reader: ReadActor::Anonymous {},
        })
        .await?;
    assert_eq!(shown.thread.orchestrator.ok_or("winner")?.identity, winner);
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert_eq!(
        read.participants(&fixture.root)
            .await?
            .into_iter()
            .filter(|participant| participant.is_open()
                && participant.role == ParticipantRole::Orchestrator)
            .count(),
        1
    );
    read.finish().await?;
    fixture.stop().await
}
