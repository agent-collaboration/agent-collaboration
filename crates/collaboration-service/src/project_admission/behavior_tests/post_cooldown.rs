use super::*;
use sqlx::{ConnectOptions, Connection};

fn root_operation(post: PostId, about: ThreadSubject) -> Result<BoardOperation, TestError> {
    Ok(BoardOperation::ThreadCreate {
        post_id: post,
        about,
        text: PostText::try_from("cooldown root".to_owned())?,
        references: Vec::new(),
        participation: None,
        watch: false,
        addressees: Vec::new(),
    })
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Corrupt stored Router time is refused before any new admission decision.
async fn malformed_cooldown_post_timestamps_are_rejected_as_attributed_invalid_records()
-> Result<(), TestError> {
    for timestamp in [-1_i64, i64::MIN, i64::MAX] {
        let fixture = SubscriptionFixture::start().await?;
        let post = PostId::try_from(fixture.root.as_str().to_owned())?;
        let mut raw = sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.project_store().await?.path())
            .connect()
            .await?;
        assert_eq!(
            sqlx::query("UPDATE posts SET router_time=? WHERE post_id=?")
                .bind(timestamp)
                .bind(post.as_str())
                .execute(&mut raw)
                .await?
                .rows_affected(),
            1
        );
        raw.close().await?;
        let request = ThreadCreateRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            actor: human("owner")?,
            acting_for: None,
            project_id: fixture.project.clone(),
            post_id: PostId::generate(),
            about: fixture.discussion_subject.clone(),
            text: PostText::try_from("must reject malformed timestamp".to_owned())?,
            references: PostReferences::try_from(Vec::new())?,
            role: None,
            watch: false,
            addressees: Vec::new(),
        };
        let error = fixture
            .application
            .board()
            .thread_create(request)
            .await
            .expect_err("corrupt timestamp was accepted");
        assert_eq!(error.kind, BoardFailureKind::InvalidRecord);
        assert_eq!(error.stage, BoardFailureStage::Inspection);
        assert_eq!(error.next_action, BoardNextAction::InspectResource);
        assert_eq!(
            error.details,
            BoardErrorDetails::Resource {
                resource: ResourceIdentity::Post { post_id: post }
            }
        );
        fixture.stop().await?;
    }
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Exact times enter the unchanged real admission seam.
async fn cooldown_allows_a_top_level_post_at_the_sixty_second_boundary() -> Result<(), TestError> {
    let fixture = SubscriptionFixture::start().await?;
    let post = PostId::generate();
    assert_accepted(
        &fixture
            .submit(
                human("owner")?,
                root_operation(post.clone(), fixture.discussion_subject.clone())?,
                fixture.now + Duration::seconds(60),
            )
            .await?,
    );
    let mut read = fixture.project_store().await?.snapshot().await?;
    let record = read.record(&post.into()).await?.ok_or("post")?;
    assert!(matches!(record, Record::Post(post) if post.project_id == fixture.project));
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Per-project cooldown preserves the original 60-second window and exact remaining time.
async fn thirty_seconds_is_still_inside_the_project_cooldown() -> Result<(), TestError> {
    let fixture = SubscriptionFixture::start().await?;
    let reply = fixture
        .submit(
            human("owner")?,
            root_operation(PostId::generate(), fixture.discussion_subject.clone())?,
            fixture.now + Duration::seconds(30),
        )
        .await?;
    assert_eq!(
        refusals(&reply)?,
        &[Refusal::TopLevelPostCooldown {
            retry_after_seconds: PostCooldownSeconds::try_from(30)?
        }]
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Concurrent requests race at the actual per-project writer gate.
async fn competing_top_level_posts_for_one_actor_commit_only_once() -> Result<(), TestError> {
    let fixture = SubscriptionFixture::empty().await?;
    let first = PostId::generate();
    let second = PostId::generate();
    let (left, right) = tokio::join!(
        fixture.submit(
            human("same-actor")?,
            root_operation(first.clone(), fixture.discussion_subject.clone())?,
            fixture.now
        ),
        fixture.submit(
            human("same-actor")?,
            root_operation(second.clone(), fixture.discussion_subject.clone())?,
            fixture.now
        )
    );
    let left = left?;
    let right = right?;
    let successful = |reply: &WriteReply| {
        matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        )
    };
    assert_eq!(
        usize::from(successful(&left)) + usize::from(successful(&right)),
        1
    );
    let denied = if successful(&left) { &right } else { &left };
    assert_eq!(
        refusals(denied)?,
        &[Refusal::TopLevelPostCooldown {
            retry_after_seconds: PostCooldownSeconds::try_from(60)?
        }]
    );
    let mut read = fixture.project_store().await?.snapshot().await?;
    let posts = read.records(RecordKind::Post).await?;
    assert_eq!(posts.len(), 1);
    assert!(
        matches!(posts.first(), Some(Record::Post(post)) if post.post_id == first || post.post_id == second)
    );
    read.finish().await?;
    fixture.stop().await
}
