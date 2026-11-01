//! Confirms the credential, public metadata and retirement before a login becomes usable.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use super::{
    device_code_flow::RefreshCredentialBundle,
    login_contract::{FleetLoginUnknown, RefreshCredentialKeeping},
    login_files::{MachineLoginMetadata, persist_metadata_with_sync},
    refresh_credential_store::{RefreshCredentialError, RefreshCredentialStore},
};

pub(super) struct MachineLoginPublication {
    pub root: PathBuf,
    pub credentials: Arc<RefreshCredentialStore>,
    pub sync_directory: fn(&Path) -> std::io::Result<()>,
}

pub(super) fn sync_directory(directory: &Path) -> std::io::Result<()> {
    std::fs::File::open(directory)?.sync_all()
}

impl MachineLoginPublication {
    pub(super) fn publish_login(
        &self,
        metadata: &MachineLoginMetadata,
        credential: &RefreshCredentialBundle,
        keeping: RefreshCredentialKeeping,
    ) -> Result<(), FleetLoginUnknown> {
        let unknown = || FleetLoginUnknown::CredentialPublication {
            organization_id: metadata.organization_id.clone(),
            enrollment_id: metadata.enrollment_id.clone(),
            keeping,
        };
        // Issuance may already have superseded the previous enrollment. Even an
        // early local write failure therefore cannot promise a refused login.
        self.credentials.write(credential).map_err(|_| unknown())?;
        persist_metadata_with_sync(&self.root, metadata, self.sync_directory)
            .map_err(|_| unknown())?;
        self.credentials
            .retire_earlier_machine_credentials(&metadata.enrollment_id)
            .map_err(|_| unknown())?;
        Ok(())
    }

    pub(super) fn publish_rotation(
        &self,
        credential: &RefreshCredentialBundle,
    ) -> Result<(), RefreshCredentialError> {
        self.credentials.write(credential)
    }
}
