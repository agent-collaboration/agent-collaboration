//! Explicit accepted-directory and persisted native-grant inputs for board CRUD fixtures.
use super::{TestError, fixture_peer};
use collaboration_service::{FleetClient, ProjectStoreRegistry};
use message_board::*;
use runtime_primitives::Clock;

pub async fn prepare_registered_creation(
    fleet: &FleetClient,
    registry: &ProjectStoreRegistry,
    request: &ProjectCreateRequest,
    origin: &ActorOrigin,
    clock: &dyn Clock,
) -> Result<(), TestError> {
    settle_accepted_creation(fleet, request, origin).await?;
    let store = registry.open_project(&request.project_id).await?;
    match store.registration_outcome().await? {
        fleet_sync_writer::WriterRegistrationOutcome::Ready { .. } => return Ok(()),
        fleet_sync_writer::WriterRegistrationOutcome::Pending => {}
        fleet_sync_writer::WriterRegistrationOutcome::Failed { reason } => {
            return Err(format!("fixture registration failed: {reason:?}").into());
        }
        fleet_sync_writer::WriterRegistrationOutcome::Refused { .. } => {
            return Err("fixture registration is refused".into());
        }
    }
    let now = clock.now();
    registry
        .database(&request.project_id)
        .await?
        .observe_registration(
            fleet_sync_protocol::LeaseStatus {
                database: fleet_sync_protocol::DatabaseId::new(uuid::Uuid::parse_str(
                    request.project_id.as_str(),
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

/// The full creating input is queued before actual central acceptance supplies the directory.
pub async fn settle_accepted_creation(
    fleet: &FleetClient,
    request: &ProjectCreateRequest,
    origin: &ActorOrigin,
) -> Result<(), TestError> {
    let Some(Record::Board(board)) = fleet.record(&request.board_id.clone().into()).await? else {
        return Err("registered creation fixture requires its board".into());
    };
    let reply = fleet
        .submit_creation(
            request,
            &fleet_admission::FleetRequest {
                request_id: request.request_id.clone(),
                workspace: board.workspace_id,
                actor: RecordedActor::declared(request.actor.clone()),
                acting_for: request.acting_for.clone(),
                protocol_version: request.protocol_version,
                operation: fleet_admission::FleetOperation::ProjectDirectoryCreate {
                    project: request.project_id.clone(),
                    board: request.board_id.clone(),
                    title: request.title.clone(),
                },
            },
            &fleet_admission::FleetIds {
                generated_record: RecordId::generate(),
            },
            origin,
        )
        .await?;
    let RequestResult::Decided(decision @ RequestDecision::Accepted { .. }) = reply.result else {
        return Err("registered creation fixture directory was refused".into());
    };
    // Replay the actual central decision through the idempotent settlement owner.
    fleet
        .settle_central_reply(&request.request_id, &decision)
        .await?;
    Ok(())
}
