//! Self-hosted credentials use the same protected store as Fleet credentials.
use crate::model::{SecretKey, SecretStoreError};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

#[derive(Clone, Copy)]
pub enum CentralSecretKind {
    SigningKey,
    OwnerCredential,
    EnrollmentCredential,
}
impl CentralSecretKind {
    pub fn secret_key(self) -> Result<SecretKey, SecretStoreError> {
        SecretKey::new(match self {
            Self::SigningKey => "central_signing_key",
            Self::OwnerCredential => "central_owner_credential",
            Self::EnrollmentCredential => "central_enrollment_credential",
        })
    }
    pub(crate) fn parse(key: &SecretKey) -> Result<Option<Self>, SecretStoreError> {
        Ok(match key.as_str() {
            "central_signing_key" => Some(Self::SigningKey),
            "central_owner_credential" => Some(Self::OwnerCredential),
            "central_enrollment_credential" => Some(Self::EnrollmentCredential),
            name if name.starts_with("central_") => {
                return Err(SecretStoreError::InvalidCentralSecretKey);
            }
            _ => None,
        })
    }
}

/// Serialization is used only for encrypted storage and explicit private handoff.
/// Deliberately has no Debug implementation.
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CentralCredential {
    Owner {
        endpoint: String,
        assertion: String,
        expires_at: String,
    },
    Enrollment {
        endpoint: String,
        owner_assertion: String,
        owner_expires_at: String,
        client_id: String,
        assertion: String,
        expires_at: String,
    },
}

impl CentralCredential {
    pub fn encode_secret(
        &self,
    ) -> Result<codex_router_core::redaction::SecretString, CredentialFileError> {
        Ok(codex_router_core::redaction::SecretString::new(
            serde_json::to_string(self).map_err(|_| CredentialFileError::Malformed)?,
        ))
    }
    pub fn decode_secret(
        value: &codex_router_core::redaction::SecretString,
    ) -> Result<Self, CredentialFileError> {
        serde_json::from_str(value.expose_secret()).map_err(|_| CredentialFileError::Malformed)
    }
    pub fn write_private_file(&self, path: &Path) -> Result<(), CredentialFileError> {
        if !path.is_absolute() {
            return Err(CredentialFileError::Permissions);
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(self.encode_secret()?.expose_secret().as_bytes())?;
        file.sync_all()?;
        Ok(())
    }
    /// Consumes a bounded regular 0600 file owned by the current OS account.
    pub fn consume_private_file(path: &Path) -> Result<Self, CredentialFileError> {
        if !path.is_absolute() {
            return Err(CredentialFileError::Permissions);
        }
        let before = std::fs::symlink_metadata(path)?;
        let mut file = File::open(path)?;
        let metadata = file.metadata()?;
        let account = rustix::process::geteuid().as_raw();
        if !before.is_file()
            || !metadata.is_file()
            || before.ino() != metadata.ino()
            || before.dev() != metadata.dev()
            || metadata.mode() & 0o777 != 0o600
            || metadata.uid() != account
            || metadata.nlink() != 1
            || metadata.len() > 65536
        {
            return Err(CredentialFileError::Permissions);
        }
        let mut contents = String::new();
        Read::by_ref(&mut file)
            .take(65537)
            .read_to_string(&mut contents)?;
        let credential =
            serde_json::from_str(&contents).map_err(|_| CredentialFileError::Malformed)?;
        let current = std::fs::symlink_metadata(path)?;
        if current.ino() != metadata.ino() || current.dev() != metadata.dev() {
            return Err(CredentialFileError::Permissions);
        }
        std::fs::remove_file(path)?;
        Ok(credential)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CredentialFileError {
    #[error(
        "central credential handoff requires a bounded regular 0600 file owned by this account"
    )]
    Permissions,
    #[error("central credential is malformed")]
    Malformed,
    #[error("central credential file operation failed")]
    Io(#[from] std::io::Error),
}
