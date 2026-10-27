#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
use board_fixture::fixture_peer;
use collaboration_runtime::ProjectStoreRegistry;
use message_board::ProjectId;
use std::sync::Arc;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn empty_registration_refusals_reopen_for_metadata_and_cannot_be_opened_for_writing()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let registry = ProjectStoreRegistry::new(
        directory.path(),
        Some(fixture_peer()?),
        Arc::new(runtime_primitives::SystemClock),
    );
    let store = registry.open_project(&project).await?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.high_water().position,
        message_board::FeedPosition::ZERO
    );
    assert!(read.record(&project.clone().into()).await?.is_none());
    read.finish().await?;
    let database = registry.database(&project).await?;
    database
        .reject_registration(fleet_sync_protocol::GateReason {
            code: fleet_sync_protocol::GateReasonCode::new("board.projectRejected")?,
            detail: "native orphan fixture".to_owned(),
        })
        .await?;
    registry.orphan_project(&project).await?;
    assert!(registry.open_project(&project).await.is_err());
    assert!(registry.existing_project(&project).await?.is_none());
    assert!(registry.rejected_project(&project).await?.is_some());
    let reopened = ProjectStoreRegistry::new(
        directory.path(),
        Some(fixture_peer()?),
        Arc::new(runtime_primitives::SystemClock),
    );
    assert!(reopened.rejected_project(&project).await?.is_some());
    assert!(reopened.open_project(&project).await.is_err());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn registry_opens_one_native_handle_per_project_and_isolates_writer_locks()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let registry = Arc::new(ProjectStoreRegistry::new(
        directory.path(),
        Some(fixture_peer()?),
        Arc::new(runtime_primitives::SystemClock),
    ));
    let first_id = ProjectId::generate();
    let second_id = ProjectId::generate();
    let (first, again) = tokio::try_join!(
        registry.open_project(&first_id),
        registry.open_project(&first_id)
    )?;
    assert!(Arc::ptr_eq(&first, &again));
    let second = registry.open_project(&second_id).await?;
    grant(&registry, &first_id).await?;
    grant(&registry, &second_id).await?;
    assert_ne!(first.path(), second.path());
    assert_eq!(
        first.path(),
        directory
            .path()
            .join("projects")
            .join(first_id.as_str())
            .join("e1.sqlite")
    );
    let held = first.begin_admission(origin()?).await?;
    let independent = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        second.begin_admission(origin()?),
    )
    .await??;
    independent.rollback().await?;
    held.rollback().await?;
    Ok(())
}

async fn grant(
    registry: &ProjectStoreRegistry,
    project: &ProjectId,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use runtime_primitives::{Clock, SystemClock};
    let now = SystemClock.now();
    registry
        .database(project)
        .await?
        .observe_registration(
            fleet_sync_protocol::LeaseStatus {
                database: fleet_sync_protocol::DatabaseId::new(uuid::Uuid::parse_str(
                    project.as_str(),
                )?)?,
                epoch: fleet_sync_protocol::Epoch::new(1)?,
                holder: fixture_peer()?,
                state: fleet_sync_protocol::LeaseState::Held,
                expires_at: now.after(board_fleet_sync::BOARD_LEASE_POLICY.lease_duration),
                remaining_ms: u64::try_from(
                    board_fleet_sync::BOARD_LEASE_POLICY
                        .lease_duration
                        .as_millis(),
                )?,
            },
            now,
        )
        .await?;
    Ok(())
}

fn origin() -> Result<message_board::ActorOrigin, Box<dyn std::error::Error + Send + Sync>> {
    Ok(message_board::ActorOrigin {
        installation: message_board::ServiceId::try_from(
            "00000000-0000-4000-8000-000000000001".to_owned(),
        )?,
        machine: message_board::MachineLabel::try_from("registry fixture".to_owned())?,
    })
}
