//! Typed central-answer fixtures are persisted and interpreted by the actual library owner.
use collaboration_service::ProjectStoreRegistry;
use fleet_sync_protocol::{LeaseStatus, RenewAnswer, RenewResult};
use message_board::ProjectId;
use runtime_primitives::SyncTime;

type FixtureError = Box<dyn std::error::Error + Send + Sync>;

pub async fn observe_same_epoch_renewal(
    registry: &ProjectStoreRegistry,
    project: &ProjectId,
    previous: LeaseStatus,
    started: SyncTime,
) -> Result<(), FixtureError> {
    let expires_at = started.after(board_fleet_sync::BOARD_LEASE_POLICY.lease_duration);
    registry
        .database(project)
        .await?
        .observe_renewal(
            RenewAnswer::Renewed {
                recorded: RenewResult {
                    epoch: previous.epoch,
                    expires_at,
                },
                current: LeaseStatus {
                    expires_at,
                    remaining_ms: 14_400_000,
                    ..previous
                },
                transfer_in_progress: None,
                replayed: false,
            },
            started,
        )
        .await?;
    Ok(())
}
