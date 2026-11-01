//! Confirms credential bytes, the public repository row, and retirement in that order.
use std::sync::Arc;

use codex_router_fleet_state::fleet_login::{FleetLoginCandidate, SavedFleetLogin};
use codex_router_fleet_state::fleet_login_repository::FleetLoginRepository;

use super::{
    device_code_flow::RefreshCredentialBundle,
    login_contract::{FleetLoginUnknown, RefreshCredentialKeeping},
    refresh_credential_store::{RefreshCredentialError, RefreshCredentialStore},
};

pub(super) struct MachineLoginPublication {
    pub repository: Arc<dyn FleetLoginRepository>,
    pub credentials: Arc<RefreshCredentialStore>,
    #[cfg(any(test, feature = "runtime-test-support"))]
    pub after_commit: Option<fn() -> Result<(), ()>>,
}
impl MachineLoginPublication {
    pub(super) async fn publish_login(
        &self,
        candidate: &FleetLoginCandidate,
        credential: &RefreshCredentialBundle,
        keeping: RefreshCredentialKeeping,
    ) -> Result<SavedFleetLogin, FleetLoginUnknown> {
        let unknown = || FleetLoginUnknown::CredentialPublication {
            organization_id: candidate.organization_id.clone(),
            enrollment_id: candidate.enrollment_id.clone(),
            keeping,
        };
        self.credentials.write(credential).map_err(|_| unknown())?;
        let saved = self
            .repository
            .replace_fleet_login(candidate)
            .await
            .map_err(|_| unknown())?;
        #[cfg(any(test, feature = "runtime-test-support"))]
        if let Some(after_commit) = self.after_commit {
            after_commit().map_err(|()| unknown())?;
        }
        self.credentials
            .retire_earlier_machine_credentials(&saved.enrollment_id)
            .map_err(|_| unknown())?;
        Ok(saved)
    }
    pub(super) fn publish_rotation(
        &self,
        credential: &RefreshCredentialBundle,
    ) -> Result<(), RefreshCredentialError> {
        self.credentials.write(credential)
    }
}
