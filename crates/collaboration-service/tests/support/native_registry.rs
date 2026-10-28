//! Explicit fixture enrollment and the real native registry, shared by surface tests.
use collaboration_runtime::ProjectStoreRegistry;
use message_board::ProjectId;
use std::sync::Arc;

type FixtureError = Box<dyn std::error::Error + Send + Sync>;

pub fn new_registry(root: &std::path::Path) -> Result<Arc<ProjectStoreRegistry>, FixtureError> {
    new_registry_with_clock(root, Arc::new(runtime_primitives::SystemClock))
}

pub fn new_registry_with_clock(
    root: &std::path::Path,
    clock: Arc<dyn runtime_primitives::Clock>,
) -> Result<Arc<ProjectStoreRegistry>, FixtureError> {
    Ok(Arc::new(ProjectStoreRegistry::new(
        root,
        Some(fixture_peer()?),
        clock,
    )))
}

pub fn fixture_peer() -> Result<fleet_sync_protocol::PeerId, FixtureError> {
    Ok(fleet_sync_protocol::PeerId::try_from(
        "0192a1b2-c3d4-7000-8000-000000000001".to_owned(),
    )?)
}

/// Local fleet policy is a stand-in; A3c separately observes native registration.
pub struct PendingRegistration;
impl collaboration_contracts::ProjectRegistration for PendingRegistration {
    fn wait_ready<'runtime>(
        &'runtime self,
        _: &'runtime ProjectId,
    ) -> collaboration_contracts::ProjectRegistrationFuture<'runtime> {
        Box::pin(async { Ok(collaboration_contracts::ProjectRegistrationState::Pending) })
    }

    fn start<'runtime>(
        &'runtime self,
        _: &'runtime ProjectId,
    ) -> collaboration_contracts::ProjectRegistrationFuture<'runtime> {
        Box::pin(async { Ok(collaboration_contracts::ProjectRegistrationState::Pending) })
    }
}

/// MCP component fixtures supply accepted directory policy and a real persisted grant.
// This source module is also shared by integration targets that do not use this arranger.
pub async fn prepare_registered_creation(
    fleet: &collaboration_runtime::FleetClient,
    registry: &ProjectStoreRegistry,
    input: &message_board::ProjectCreateRequest,
    origin: &message_board::ActorOrigin,
) -> Result<(), FixtureError> {
    prepare_registered_creation_for_peer(fleet, registry, input, origin, fixture_peer()?).await
}

pub async fn prepare_registered_creation_for_peer(
    fleet: &collaboration_runtime::FleetClient,
    registry: &ProjectStoreRegistry,
    input: &message_board::ProjectCreateRequest,
    origin: &message_board::ActorOrigin,
    peer: fleet_sync_protocol::PeerId,
) -> Result<(), FixtureError> {
    use fleet_admission::{FleetOperation, FleetRequest};
    use message_board::{Record, RecordedActor, RequestDecision, RequestResult};
    use runtime_primitives::{Clock, SystemClock};
    let Some(Record::Board(board)) = fleet.record(&input.board_id.clone().into()).await? else {
        return Err("fixture board missing".into());
    };
    let directory = FleetRequest {
        request_id: input.request_id.clone(),
        workspace: board.workspace_id,
        actor: RecordedActor::declared(input.actor.clone()),
        acting_for: input.acting_for.clone(),
        protocol_version: input.protocol_version,
        operation: FleetOperation::ProjectDirectoryCreate {
            project: input.project_id.clone(),
            board: input.board_id.clone(),
            title: input.title.clone(),
        },
    };
    let reply = fleet.submit_creation(input, &directory, origin).await?;
    match reply.result {
        RequestResult::Decided(RequestDecision::Accepted { .. }) => {}
        RequestResult::PendingUnreachable => {
            let queued = fleet
                .pending_request(&input.request_id)
                .await?
                .ok_or("creating input was not queued")?;
            let reply = fleet.submit(&queued.request, origin).await?;
            let RequestResult::Decided(decision @ RequestDecision::Accepted { .. }) = reply.result
            else {
                return Err("fixture directory refused".into());
            };
            // The stand-in's policy answer is supplied explicitly as central's
            // typed answer. A real session's Accepted needs no such drive.
            fleet
                .settle_central_reply(&input.request_id, &decision)
                .await?;
        }
        result => return Err(format!("fixture creation was not accepted: {result:?}").into()),
    }
    let store = registry.open_project(&input.project_id).await?;
    match store.registration_outcome().await? {
        fleet_sync_writer::WriterRegistrationOutcome::Ready { .. } => return Ok(()),
        fleet_sync_writer::WriterRegistrationOutcome::Pending => {}
        outcome => {
            return Err(format!("fixture registration cannot receive a grant: {outcome:?}").into());
        }
    }
    let now = SystemClock.now();
    registry
        .database(&input.project_id)
        .await?
        .observe_registration(
            fleet_sync_protocol::LeaseStatus {
                database: fleet_sync_protocol::DatabaseId::new(uuid::Uuid::parse_str(
                    input.project_id.as_str(),
                )?)?,
                epoch: fleet_sync_protocol::Epoch::new(1)?,
                holder: peer,
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
