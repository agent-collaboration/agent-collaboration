//! Native test composition with explicit directory and registration inputs.
use collaboration_contracts::ProjectRegistration;
use collaboration_contracts::ProjectRegistrationFuture;
use collaboration_contracts::ProjectRegistrationState;
use collaboration_runtime::ProjectStoreRegistry;
use message_board::*;
use std::{path::Path, sync::Arc};

type FixtureError = Box<dyn std::error::Error + Send + Sync>;

pub(crate) fn registry(
    root: &Path,
    clock: Arc<dyn runtime_primitives::Clock>,
) -> Result<Arc<ProjectStoreRegistry>, FixtureError> {
    let peer = fleet_sync_protocol::PeerId::new(uuid::Uuid::parse_str(
        "0192a1b2-c3d4-7000-8000-000000000001",
    )?)?;
    Ok(Arc::new(ProjectStoreRegistry::new(
        root,
        Some(peer),
        clock,
        message_board_storage::CursorSigningKey::try_from(vec![0x5a_u8; 32])?,
    )))
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
