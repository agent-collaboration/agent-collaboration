use super::*;

async fn archive(fixture: &SubscriptionFixture) -> Result<(), TestError> {
    let mut snapshot = fixture.project_store().await?.snapshot().await?;
    let project = snapshot
        .record(&fixture.project.clone().into())
        .await?
        .ok_or("project")?;
    let board = project.envelope().scope.board.clone().ok_or("board")?;
    snapshot.finish().await?;
    super::subscription_fixture::assert_public_accepted(&fixture.application.board().board_archive(serde_json::from_value(serde_json::json!({"requestId":RequestId::generate(),"protocolVersion":1,"actingFor":null,"actor":human("owner")?,"boardId":board}))?).await?);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Archive exceptions are narrow; Result propagates native setup errors."
)]
async fn archived_reader_opt_out_and_keyed_marks_pass_while_work_and_new_interest_are_refused()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: SubscriptionScope::project(fixture.project.clone()),
                policy: SubscriptionPolicyPatch::default(),
            },
            fixture.now,
        )
        .await?;
    archive(&fixture).await?;
    for operation in [
        BoardOperation::ThreadJoin {
            thread_id: fixture.root.clone(),
            role: ParticipantRole::Participant,
            watch: true,
            mode: None,
            when_idle: None,
            replace: None,
            note: None,
        },
        BoardOperation::ThreadSubscribe {
            scope: SubscriptionScope::thread(fixture.root.clone()),
            policy: SubscriptionPolicyPatch::default(),
        },
        BoardOperation::ThreadRenew {
            scope: SubscriptionScope::thread(fixture.root.clone()),
        },
        BoardOperation::ThreadWatch {
            thread_id: fixture.root.clone(),
        },
        BoardOperation::ProjectWatch {},
        BoardOperation::ThreadPost {
            post_id: PostId::generate(),
            thread_id: fixture.root.clone(),
            text: PostText::try_from("refused".to_owned())?,
            references: Vec::new(),
            addressees: Vec::new(),
        },
    ] {
        let reply = fixture
            .submit(reader.clone(), operation, fixture.now)
            .await?;
        assert_eq!(refusals(&reply)?, &[Refusal::Archived]);
    }
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadUnsubscribe {
                scope: SubscriptionScope::thread(fixture.root.clone()),
            },
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadUnwatch {
                thread_id: fixture.root.clone(),
            },
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ProjectUnwatch {},
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ReadMarkInitialize {
                reader: reader.clone(),
                scope: ReadScope::Project {
                    project_id: fixture.project.clone(),
                },
            },
            fixture.now,
        )
        .await?;
    let snapshot = fixture.project_store().await?.snapshot().await?;
    let through = snapshot.high_water();
    snapshot.finish().await?;
    let request = ProjectRequest {
        request_id: RequestId::generate(),
        project_id: fixture.project.clone(),
        actor: reader.clone(),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: BoardOperation::ReadMarkAdvance {
            reader: reader.clone(),
            scope: ReadScope::Thread {
                thread_id: fixture.root.clone(),
            },
            through,
        },
    };
    let first = fixture.admission.submit_at(&request, fixture.now).await?;
    assert_accepted(&first);
    let snapshot = fixture.project_store().await?.snapshot().await?;
    let after = snapshot.high_water();
    snapshot.finish().await?;
    let repeated = fixture.admission.submit_at(&request, fixture.now).await?;
    assert_eq!(repeated.result, first.result);
    let mut snapshot = fixture.project_store().await?.snapshot().await?;
    assert_eq!(snapshot.high_water(), after);
    assert!(
        snapshot
            .effective_thread_watch(&fixture.root, &reader)
            .await?
            .is_none()
    );
    assert!(snapshot.project_watch(&reader).await?.is_none());
    assert_eq!(
        snapshot
            .read_mark(
                &reader,
                &ReadScope::Thread {
                    thread_id: fixture.root.clone()
                }
            )
            .await?
            .ok_or("mark")?
            .through,
        through
    );
    snapshot.finish().await?;
    fixture.stop().await
}
