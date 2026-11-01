//! The refresh credential and its public client identifier share one protected envelope.
use std::sync::Arc;

use codex_router_core::redaction::SecretString;
use codex_router_secret_store::{
    SecretStore,
    classified_store_open::{ClassifiedCredentialStore, UnusableCause},
    encrypted_credential_store::EncryptedCredentialStore,
    fleet_secret_key::{FleetSecretKey, FleetSecretKind},
    model::SecretKey,
};
use fleet_identity_protocol::{EnrollmentId, InstallationId};
use serde::{Deserialize, Serialize};

use super::{device_code_flow::RefreshCredentialBundle, login_contract::RefreshCredentialKeeping};

pub(super) enum RefreshCredentialStore {
    Encrypted {
        store: Arc<EncryptedCredentialStore>,
        installation: InstallationId,
    },
    MemoryOnly,
    Unusable {
        cause: UnusableCause,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum RefreshCredentialError {
    #[error("the encrypted credential store is unusable")]
    Unusable { cause: UnusableCause },
    #[error("the machine's refresh credential is damaged or unavailable")]
    DamagedCredential,
    #[error("the machine's refresh credential could not be stored")]
    WriteFailed,
}

impl RefreshCredentialStore {
    pub(super) fn from_classification(
        classified: ClassifiedCredentialStore,
        installation: InstallationId,
    ) -> Self {
        match classified {
            ClassifiedCredentialStore::Usable(store) => Self::Encrypted {
                store: Arc::new(store),
                installation,
            },
            ClassifiedCredentialStore::Absent => Self::MemoryOnly,
            ClassifiedCredentialStore::Unusable { cause } => Self::Unusable { cause },
        }
    }

    pub(super) fn keeping(&self) -> Result<RefreshCredentialKeeping, RefreshCredentialError> {
        match self {
            Self::Encrypted { .. } => Ok(RefreshCredentialKeeping::EncryptedStore),
            Self::MemoryOnly => Ok(RefreshCredentialKeeping::MemoryOnly),
            Self::Unusable { cause } => Err(RefreshCredentialError::Unusable { cause: *cause }),
        }
    }

    pub(super) fn read(
        &self,
        enrollment: &EnrollmentId,
    ) -> Result<Option<RefreshCredentialBundle>, RefreshCredentialError> {
        match self {
            Self::MemoryOnly => Ok(None),
            Self::Unusable { cause } => Err(RefreshCredentialError::Unusable { cause: *cause }),
            Self::Encrypted {
                store,
                installation,
            } => {
                let key = credential_key(installation, enrollment)?;
                let secret = match store.read_secret(&key) {
                    Ok(secret) => secret,
                    Err(codex_router_secret_store::model::SecretStoreError::Filesystem {
                        source,
                        ..
                    }) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(_) => return Err(RefreshCredentialError::DamagedCredential),
                };
                let wire: StoredRefreshCredential = serde_json::from_str(secret.expose_secret())
                    .map_err(|_| RefreshCredentialError::DamagedCredential)?;
                if wire.machine_client_id != *enrollment || wire.refresh_credential.is_empty() {
                    return Err(RefreshCredentialError::DamagedCredential);
                }
                Ok(Some(RefreshCredentialBundle {
                    machine_client_id: wire.machine_client_id,
                    refresh_credential: SecretString::new(wire.refresh_credential),
                }))
            }
        }
    }

    pub(super) fn write(
        &self,
        credential: &RefreshCredentialBundle,
    ) -> Result<(), RefreshCredentialError> {
        match self {
            Self::MemoryOnly => Ok(()),
            Self::Unusable { cause } => Err(RefreshCredentialError::Unusable { cause: *cause }),
            Self::Encrypted {
                store,
                installation,
            } => {
                // SecretString's ordinary serializer is redacted. Only this
                // explicit encryption boundary exposes its plaintext value.
                let wire = StoredRefreshCredential {
                    machine_client_id: credential.machine_client_id.clone(),
                    refresh_credential: credential.refresh_credential.expose_secret().to_owned(),
                };
                let encoded = serde_json::to_string(&wire)
                    .map_err(|_| RefreshCredentialError::WriteFailed)?;
                store
                    .write_staged_confirmed(
                        &credential_key(installation, &credential.machine_client_id)?,
                        &SecretString::new(encoded),
                    )
                    .map_err(|_| RefreshCredentialError::WriteFailed)
            }
        }
    }

    pub(super) fn retire_earlier_machine_credentials(
        &self,
        enrollment: &EnrollmentId,
    ) -> Result<(), RefreshCredentialError> {
        match self {
            Self::MemoryOnly => Ok(()),
            Self::Unusable { cause } => Err(RefreshCredentialError::Unusable { cause: *cause }),
            Self::Encrypted {
                store,
                installation,
            } => {
                let active = FleetSecretKey::new(
                    FleetSecretKind::MachineCredential,
                    installation.as_str(),
                    enrollment.as_str(),
                )
                .map_err(|_| RefreshCredentialError::DamagedCredential)?;
                store
                    .remove_retired_machine_credentials(&active)
                    .map_err(|_| RefreshCredentialError::WriteFailed)
            }
        }
    }
}

fn credential_key(
    installation: &InstallationId,
    enrollment: &EnrollmentId,
) -> Result<SecretKey, RefreshCredentialError> {
    FleetSecretKey::new(
        FleetSecretKind::MachineCredential,
        installation.as_str(),
        enrollment.as_str(),
    )
    .and_then(|identity| identity.secret_key())
    .map_err(|_| RefreshCredentialError::DamagedCredential)
}

// This format exists only inside the existing authenticated encryption envelope.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredRefreshCredential {
    machine_client_id: EnrollmentId,
    refresh_credential: String,
}

#[cfg(test)]
#[path = "refresh_credential_store_tests.rs"]
mod tests;
