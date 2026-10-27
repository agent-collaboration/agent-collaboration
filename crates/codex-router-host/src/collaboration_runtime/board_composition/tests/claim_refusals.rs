//! Named claim refusals come from actual canonical Registry/Fleet/Copy owners.
use super::*;
use board_central_test_support::{
    ConformanceBoardState, DispatchFaultObservation, ObjectAddress, RuntimeControl,
};
use board_sync_protocol::{ProjectClaimRefusal, ProjectClaimReply};
use fleet_sync_protocol::{DatabaseId, Epoch, TransferId};

use super::claim_fixture::ClaimFixture;
#[path = "claim_future_schema.rs"]
mod future_schema;

#[tokio::test]
async fn claim_not_found_and_archived_refusals_create_no_local_writer() -> TestResult {
    let fixture = Box::pin(ClaimFixture::start()).await?;
    let proof = async {
        let missing = ProjectId::generate();
        assert_eq!(
            fixture.claim(&missing).await?,
            ProjectClaimReply::Refused(ProjectClaimRefusal::NotFound)
        );
        assert!(fixture.registry.existing_project(&missing).await?.is_none());
        let archived = ProjectId::generate();
        let (manifest, pages, _) = project_claim::captured_board_snapshot(
            &fixture.central,
            &fixture.directory.path().join("archived.sqlite"),
            &archived,
        )
        .await?;
        fixture
            .central
            .import_board_snapshot_in_state(ConformanceBoardState::Archived, pages, manifest)
            .await?;
        assert_eq!(
            fixture.claim(&archived).await?,
            ProjectClaimReply::Refused(ProjectClaimRefusal::Archived)
        );
        assert!(
            fixture
                .registry
                .existing_project(&archived)
                .await?
                .is_none()
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = fixture.shutdown().await;
    proof?;
    closed?;
    Ok(())
}

#[tokio::test]
async fn claim_already_held_preserves_the_native_peer_without_another_seed() -> TestResult {
    let fixture = Box::pin(ClaimFixture::start()).await?;
    let proof = async {
        let project = ProjectId::generate();
        project_claim::imported_snapshot(
            &fixture.central,
            &fixture.directory.path().join("source.sqlite"),
            &project,
        )
        .await?;
        assert!(matches!(
            fixture.claim(&project).await?,
            ProjectClaimReply::Claimed { .. }
        ));
        let original = fixture.registry.database(&project).await?;
        let receipt = original.installed_snapshot().await?;
        assert_eq!(
            fixture.claim(&project).await?,
            ProjectClaimReply::Refused(ProjectClaimRefusal::AlreadyHeld {
                holder: fixture.projects.peer
            })
        );
        assert!(Arc::ptr_eq(
            &original,
            &fixture.registry.database(&project).await?
        ));
        assert_eq!(original.installed_snapshot().await?, receipt);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = fixture.shutdown().await;
    proof?;
    closed?;
    Ok(())
}

#[tokio::test]
async fn claim_observes_real_retirement_in_progress_then_retired_without_seeding() -> TestResult {
    let fixture = Box::pin(ClaimFixture::start()).await?;
    let project = ProjectId::generate();
    let database = DatabaseId::try_from(project.as_str().to_owned())?;
    let (manifest, pages, _) = project_claim::captured_board_snapshot(
        &fixture.central,
        &fixture.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    fixture
        .central
        .import_board_snapshot_in_state(ConformanceBoardState::Archived, pages, manifest.clone())
        .await?;
    fixture
        .central
        .runtime_control(
            ObjectAddress::Registry,
            RuntimeControl::HoldNextDispatch {
                destination: ObjectAddress::Copy(database),
            },
        )
        .await?;
    let retirement = fleet_sync_protocol::RequestId::new(durable_steps::Uuid::now_v7())?;
    let central = fixture.central.clone();
    let mut retiring = tokio::spawn(async move {
        central
            .retire_project(database, retirement, Epoch::new(1)?, manifest.position)
            .await
    });
    let mut first_reply = None;
    let proof = async {
        let observed = tokio::select! {
            observation = fixture.central.runtime_control(ObjectAddress::Registry, RuntimeControl::AwaitHeldDispatch) => observation?,
            answer = &mut retiring => {
                let answer = answer??;
                if answer["kind"] != "pending" { return Err(format!("retirement returned before selected dispatch: {answer}").into()); }
                first_reply = Some(answer);
                fixture.central.runtime_control(ObjectAddress::Registry, RuntimeControl::AwaitHeldDispatch).await?
            },
        };
        assert!(matches!(observed.dispatch, DispatchFaultObservation::Held {destination: ObjectAddress::Copy(actual), ..} if actual == database));
        assert_eq!(fixture.claim(&project).await?, ProjectClaimReply::Refused(ProjectClaimRefusal::TransferInProgress { transfer: TransferId::new(retirement.as_uuid())? }));
        assert!(fixture.registry.existing_project(&project).await?.is_none());
        fixture.central.runtime_control(ObjectAddress::Registry, RuntimeControl::ResumeDispatch).await?;
        let first = match first_reply.take() { Some(reply) => reply, None => timeout(BOUND, &mut retiring).await??? };
        let terminal = timeout(BOUND, async {
            let mut reply = first;
            while reply["kind"] == "pending" {
                reply = fixture.central.retire_project(database, retirement, Epoch::new(1)?, manifest.position).await?;
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(reply)
        }).await??;
        assert_eq!(terminal["kind"], "retired", "actual owner retirement must complete: {terminal}");
        assert_eq!(fixture.claim(&project).await?, ProjectClaimReply::Refused(ProjectClaimRefusal::Retired));
        assert!(fixture.registry.existing_project(&project).await?.is_none());
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let _ = fixture
        .central
        .runtime_control(ObjectAddress::Registry, RuntimeControl::ResumeDispatch)
        .await;
    if !retiring.is_finished() {
        let _ = timeout(BOUND, &mut retiring).await;
    }
    retiring.abort();
    drop(retiring);
    let closed = fixture.shutdown().await;
    proof?;
    closed?;
    Ok(())
}
