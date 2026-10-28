//! Native test composition with explicit directory and registration inputs.
use collaboration_contracts::ProjectRegistration;
use collaboration_contracts::ProjectRegistrationFuture;
use collaboration_contracts::ProjectRegistrationState;
use collaboration_runtime::ProjectStoreRegistry;
use message_board::*;
use std::{path::Path, sync::Arc};

type FixtureError = Box<dyn std::error::Error + Send + Sync>;

pub(crate) fn registry(root: &Path) -> Result<Arc<ProjectStoreRegistry>, FixtureError> {
    let peer = fleet_sync_protocol::PeerId::new(uuid::Uuid::parse_str(
        "0192a1b2-c3d4-7000-8000-000000000001",
    )?)?;
    Ok(Arc::new(ProjectStoreRegistry::new(
        root,
        Some(peer),
        Arc::new(runtime_primitives::SystemClock),
    )))
}

pub(crate) fn origin() -> Result<ActorOrigin, FixtureError> {
    Ok(ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("native fixture".to_owned())?,
    })
}

pub(crate) struct PendingRegistration;
impl ProjectRegistration for PendingRegistration {
    fn wait_ready<'runtime>(
        &'runtime self,
        _: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        Box::pin(async { Ok(ProjectRegistrationState::Pending) })
    }

    fn start<'runtime>(
        &'runtime self,
        _: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        Box::pin(async { Ok(ProjectRegistrationState::Pending) })
    }
}

/// Controlled directory and native-grant inputs for board-domain component tests.
pub(crate) async fn prepare_registered_creation(
    admission: &crate::ProjectAdmission,
    input: &ProjectCreateRequest,
) -> Result<(), FixtureError> {
    use fleet_admission::{FleetOperation, FleetRequest};
    use runtime_primitives::{Clock, SystemClock};
    let Some(Record::Board(board)) = admission
        .fleet
        .record(&input.board_id.clone().into())
        .await?
    else {
        return Err("fixture board missing".into());
    };
    let directory = admission
        .fleet
        .submit_creation(
            input,
            &FleetRequest {
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
            },
            &admission.record_origin(),
        )
        .await?;
    let RequestResult::Decided(decision @ RequestDecision::Accepted { .. }) = directory.result
    else {
        return Err("fixture directory refused".into());
    };
    admission
        .fleet
        .settle_central_reply(&input.request_id, &decision)
        .await?;
    admission.registry.open_project(&input.project_id).await?;
    let now = SystemClock.now();
    admission
        .registry
        .database(&input.project_id)
        .await?
        .observe_registration(
            fleet_sync_protocol::LeaseStatus {
                database: fleet_sync_protocol::DatabaseId::new(uuid::Uuid::parse_str(
                    input.project_id.as_str(),
                )?)?,
                epoch: fleet_sync_protocol::Epoch::new(1)?,
                holder: fleet_sync_protocol::PeerId::new(uuid::Uuid::parse_str(
                    "0192a1b2-c3d4-7000-8000-000000000001",
                )?)?,
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

/// Persist the registry observation through the native owner; no local authority map is used.
pub(crate) async fn expire_lease(
    registry: &ProjectStoreRegistry,
    project: &ProjectId,
) -> Result<fleet_sync_protocol::LeaseStatus, FixtureError> {
    use fleet_sync_protocol::{Epoch, LeaseState, LeaseStatus};
    use runtime_primitives::{Clock, SyncTime, SystemClock};
    let started = SyncTime::from_unix_millis(SystemClock.now().unix_millis() - 120_000);
    let lease = LeaseStatus {
        database: fleet_sync_protocol::DatabaseId::new(uuid::Uuid::parse_str(project.as_str())?)?,
        epoch: Epoch::new(1)?,
        holder: fleet_sync_protocol::PeerId::new(uuid::Uuid::parse_str(
            "0192a1b2-c3d4-7000-8000-000000000001",
        )?)?,
        state: LeaseState::Held,
        expires_at: SyncTime::from_unix_millis(started.unix_millis() + 60_000),
        remaining_ms: 60_000,
    };
    registry
        .database(project)
        .await?
        .observe_renewal(
            fleet_sync_protocol::RenewAnswer::Renewed {
                recorded: fleet_sync_protocol::RenewResult {
                    epoch: lease.epoch,
                    expires_at: lease.expires_at,
                },
                current: lease.clone(),
                transfer_in_progress: None,
                replayed: false,
            },
            started,
        )
        .await?;
    Ok(lease)
}

pub(crate) async fn renew_lease(
    registry: &ProjectStoreRegistry,
    project: &ProjectId,
    previous: fleet_sync_protocol::LeaseStatus,
) -> Result<(), FixtureError> {
    use fleet_sync_protocol::{RenewAnswer, RenewResult};
    use runtime_primitives::{Clock, SystemClock};
    let started = SystemClock.now();
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
                current: fleet_sync_protocol::LeaseStatus {
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
