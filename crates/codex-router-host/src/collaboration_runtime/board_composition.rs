//! Compose board storage and its named M2 lease/fleet stand-ins for this profile.
use collaboration_service::{
    FleetClient, LocalLeaseAuthority, MachineIdentity, ProjectAdmission, ProjectAdmissionProps,
    ProjectStoreRegistry,
};
use std::{io, path::Path, sync::Arc};
pub(super) async fn open_board_admission(
    directory: &Path,
    identity: MachineIdentity,
) -> io::Result<Arc<ProjectAdmission>> {
    let fleet = Arc::new(
        FleetClient::open_local_stand_in(directory)
            .await
            .map_err(io::Error::other)?,
    );
    let registry = Arc::new(ProjectStoreRegistry::new(directory));
    let admission = ProjectAdmission::new(ProjectAdmissionProps {
        registry,
        fleet,
        leases: Arc::new(LocalLeaseAuthority::default()),
        identity,
    })
    .map_err(io::Error::other)?;
    Ok(Arc::new(admission))
}
