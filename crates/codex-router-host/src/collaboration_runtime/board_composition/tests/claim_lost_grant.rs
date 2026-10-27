//! The actual Granted frame is lost; the public attempt remains unchanged on retry.
use super::claim_fixture::ClaimFixture;
use super::*;
use board_sync_protocol::{ProjectClaimReply, ProjectClaimRequest};
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn claim_lost_granted_frame_replays_the_same_public_attempt_and_exact_grant() -> TestResult {
    let fixture = Box::pin(ClaimFixture::start()).await?;
    let project = ProjectId::generate();
    let (prefix, expected) = project_claim::imported_snapshot(
        &fixture.central,
        &fixture.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    let api_directory = fixture.directory.path().join("api");
    std::fs::create_dir(&api_directory)?;
    std::fs::set_permissions(&api_directory, std::fs::Permissions::from_mode(0o700))?;
    let app = (collaboration_service::ApplicationFixture::new(
        "550e8400-e29b-41d4-a716-446655440000",
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_project_admission(fixture.admission.clone())
    .with_project_claim(fixture.projects.clone()))
    .build();
    let api = collaboration_mcp::test_support::ServedCollaborationApi::start(&api_directory, app)?;
    let client = api.client("claim-lost-grant").await?;
    let request = ProjectClaimRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: project.clone(),
    };
    let attempt = fleet_sync_protocol::RequestId::try_from(request.request_id.as_str().to_owned())?;
    let mut fault = fixture.central.drop_claim_grant_once(attempt)?;
    let proof = async {
        // Initial Pending is allowed, but cannot satisfy the lost-Granted proof.
        let first = timeout(std::time::Duration::from_secs(45), async {
            loop {
                match client.project_claim(request.clone()).await {
                    Ok(ProjectClaimReply::Pending {attempt: current}) if current == request.request_id => {},
                    Err(collaboration_client::BoardClientError::Rejected(error)) if error.kind == BoardFailureKind::BoardUnavailable => return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(()),
                    other => return Err(format!("claim did not lose its grant: {other:?}").into()),
                }
            }
        }).await?;
        first?;
        let dropped = timeout(BOUND, fault.wait_dropped()).await??;
        assert_eq!(dropped.attempt, attempt);
        assert_eq!(dropped.epoch, fleet_sync_protocol::Epoch::new(2)?);
        assert_eq!(dropped.start, prefix.position);
        assert!(fixture.registry.existing_project(&project).await?.is_none());
        println!("CLAIM_LOST_GRANTED actual-frame={} attempt={} epoch={} start={:?}", dropped.frame.as_uuid(), dropped.attempt.as_uuid(), dropped.epoch.value(), dropped.start);
        let replay = timeout(std::time::Duration::from_secs(45), async {
            loop {
                wait_welcome(fixture.projects.welcomed.clone()).await?;
                match client.project_claim(request.clone()).await {
                    Ok(ProjectClaimReply::Pending {attempt: current}) if current == request.request_id => {},
                    Ok(answer) => return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(answer),
                    Err(collaboration_client::BoardClientError::Rejected(error)) if error.kind == BoardFailureKind::BoardUnavailable => tokio::task::yield_now().await,
                    Err(error) => return Err(error.into()),
                }
            }
        }).await??;
        assert_eq!(replay, ProjectClaimReply::Claimed {epoch: dropped.epoch, expires_at: dropped.expires_at, start: dropped.start});
        assert_eq!(fault.observed_attempts()?, vec![attempt]);
        let store = fixture.registry.existing_project(&project).await?.ok_or("replayed claim did not publish")?;
        let mut snapshot = store.snapshot().await?;
        assert_eq!(snapshot.record(&project.clone().into()).await?, Some(expected));
        snapshot.finish().await?;
        assert!(matches!(store.writer_state().await?, fleet_sync_lease::WriterState::Writing {epoch} if epoch == dropped.epoch));
        println!("CLAIM_LOST_GRANTED same-attempt exact-grant same-epoch verified-prefix published");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    drop(fault);
    drop(client);
    let api_stopped = api.stop().await;
    let closed = fixture.shutdown().await;
    proof?;
    api_stopped?;
    closed?;
    Ok(())
}
