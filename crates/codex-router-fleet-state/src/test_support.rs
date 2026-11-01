//! Volatile repository stand-ins and reusable contracts; never a production store.
mod login_repository_contract;

pub use login_repository_contract::{FleetLoginContractFailure, verify_login_repository_contract};

use crate::{
    fleet_login::{FleetLoginCandidate, SavedFleetLogin},
    fleet_login_repository::{
        FleetLoginRepository, FleetLoginStorageFailure, FleetLoginStorageRequest,
    },
};
use fleet_identity_protocol::{EnrollmentId, MachineEnvironment};
use std::sync::Mutex;

#[derive(Default)]
pub struct MemoryFleetLoginRepository {
    saved: Mutex<Option<SavedFleetLogin>>,
}

impl MemoryFleetLoginRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl FleetLoginRepository for MemoryFleetLoginRepository {
    fn load_fleet_login(&self) -> FleetLoginStorageRequest<'_, Option<SavedFleetLogin>> {
        Box::pin(async {
            self.saved
                .lock()
                .map(|saved| saved.clone())
                .map_err(|_| FleetLoginStorageFailure::Unavailable)
        })
    }

    fn replace_fleet_login<'a>(
        &'a self,
        candidate: &'a FleetLoginCandidate,
    ) -> FleetLoginStorageRequest<'a, SavedFleetLogin> {
        Box::pin(async move {
            // One lease covers the read, organization guard, environment choice
            // and replacement, standing in for the real storage transaction.
            let mut current = self
                .saved
                .lock()
                .map_err(|_| FleetLoginStorageFailure::Unavailable)?;
            if let Some(saved) = current.as_ref()
                && saved.organization_id != candidate.organization_id
            {
                return Err(FleetLoginStorageFailure::OrganizationMismatch {
                    organization_id: saved.organization_id.clone(),
                });
            }
            let saved = SavedFleetLogin {
                sign_in: candidate.sign_in.clone(),
                organization_id: candidate.organization_id.clone(),
                enrollment_id: candidate.enrollment_id.clone(),
                machine_environment: candidate
                    .requested_machine_environment
                    .clone()
                    .or_else(|| {
                        current
                            .as_ref()
                            .map(|saved| saved.machine_environment.clone())
                    })
                    .unwrap_or(MachineEnvironment::Physical),
                central: candidate.central.clone(),
            };
            *current = Some(saved.clone());
            Ok(saved)
        })
    }

    fn delete_fleet_login<'a>(
        &'a self,
        expected_enrollment: &'a EnrollmentId,
    ) -> FleetLoginStorageRequest<'a, bool> {
        Box::pin(async move {
            let mut saved = self
                .saved
                .lock()
                .map_err(|_| FleetLoginStorageFailure::Unavailable)?;
            if saved
                .as_ref()
                .is_some_and(|row| row.enrollment_id == *expected_enrollment)
            {
                *saved = None;
                Ok(true)
            } else {
                Ok(false)
            }
        })
    }
}

#[cfg(test)]
mod tests;
