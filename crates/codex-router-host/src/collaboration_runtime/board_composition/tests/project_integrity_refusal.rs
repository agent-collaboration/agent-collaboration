//! Terminal Copy integrity refusal drains only its actual project sender.
use super::*;
use fleet_sync_protocol::{DatabaseId, PieceRefusal, TableName};
use fleet_sync_writer::{CopyFailure, SenderError};
type TestValue<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn piece_integrity_refusal_joins_its_sender_without_resend_while_another_project_replicates()
-> TestResult {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .with_test_writer()
        .try_init();
    let fixture = claim_fixture::ClaimFixture::start().await?;
    let proof = Box::pin(observe_refusal(&fixture)).await;
    let stopped = fixture.shutdown().await;
    stopped?;
    proof
}

async fn observe_refusal(fixture: &claim_fixture::ClaimFixture) -> TestResult {
    let refused = ProjectId::generate();
    let (_, expected) = project_claim::imported_snapshot(
        &fixture.central,
        &fixture.directory.path().join("source.sqlite"),
        &refused,
    )
    .await?;
    assert!(matches!(
        fixture.claim(&refused).await?,
        board_sync_protocol::ProjectClaimReply::Claimed { .. }
    ));
    super::received_grants::write_and_replicate(
        &fixture.projects,
        &fixture.registry,
        &refused,
        2,
        &expected,
        origin()?,
    )
    .await?;
    let (healthy, prefix) = create_healthy_project(fixture).await?;
    super::received_grants::write_and_replicate(
        &fixture.projects,
        &fixture.registry,
        &healthy,
        1,
        &prefix,
        origin()?,
    )
    .await?;

    let writer = fixture.projects.ensure(&refused).await?;
    let mut observation = fixture.central.refuse_piece_integrity(
        DatabaseId::try_from(refused.as_str().to_owned())?,
        TableName::new("posts")?,
        TableName::new("threads")?,
    )?;
    let Record::Project(mut changed) = expected else {
        return Err("project prefix missing".into());
    };
    changed.description =
        Description::try_from("must stop after typed integrity refusal".to_owned())?;
    let mut write = writer.store.begin_admission(origin()?).await?;
    write.append_record(Record::Project(changed)).await?;
    write.commit().await?;
    observation.wait_refused().await?;

    // This observer joins naturally completed tasks. It never sends a stop.
    let failure = timeout(BOUND, writer.join_without_stopping_for_test())
        .await
        .map_err(|_| "integrity-refused sender kept running and could resend")?
        .expect_err("terminal integrity error was discarded");
    let typed = failure
        .get_ref()
        .and_then(|error| error.downcast_ref::<CopyFailure>())
        .ok_or("typed CopyFailure lost at owned task result")?;
    assert!(matches!(&typed.error, SenderError::Refused {
        reason: PieceRefusal::IntegrityViolation { table, references }, ..
    } if table.as_str()=="posts" && references.as_str()=="threads"));
    assert_eq!(
        observation.requests(),
        1,
        "refused project resent its Piece"
    );
    super::received_grants::write_and_replicate(
        &fixture.projects,
        &fixture.registry,
        &healthy,
        1,
        &prefix,
        origin()?,
    )
    .await?;
    assert_eq!(
        observation.requests(),
        1,
        "healthy project progress restarted the refused sender"
    );
    println!(
        "PIECE_INTEGRITY_CASE typed project failure; sender joined; requests=1; second project acknowledged"
    );
    Ok(())
}

async fn create_healthy_project(
    fixture: &claim_fixture::ClaimFixture,
) -> TestValue<(ProjectId, Record)> {
    let project = ProjectId::generate();
    let creating = ProjectCreateRequest {
        request_id: message_board::RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: project.clone(),
        board_id: BoardId::try_from("019f0000-0000-7000-8000-000000005594".to_owned())?,
        title: ResourceName::try_from("healthy project".to_owned())?,
        description: None,
        actor: Identity::Human {
            human_id: HumanId::try_from("owner".to_owned())?,
        },
        acting_for: None,
    };
    timeout(BOUND, async {
        loop {
            match fixture.admission.create_project(&creating).await?.result {
                RequestResult::Decided(RequestDecision::Accepted { .. }) => {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                RequestResult::Decided(RequestDecision::Rejected { reasons }) => {
                    return Err(format!("healthy creation refused: {reasons:?}").into());
                }
                _ => tokio::task::yield_now().await,
            }
        }
    })
    .await??;
    let store =
        super::received_grants::wait_published_project(&fixture.registry, &project, 1).await?;
    let mut snapshot = store.snapshot().await?;
    let mut expected = snapshot
        .record(&project.clone().into())
        .await?
        .ok_or("created project prefix")?;
    snapshot.finish().await?;
    // Subsequent fixture writes are new admissions, not the decided creation request.
    expected.envelope_mut().request = None;
    Ok((project, expected))
}
