//! Private deployment input and public login metadata; neither file contains credentials.
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

use fleet_identity_protocol::{EnrollmentId, OrganizationId};
use fleet_sync_protocol::{PeerId, ReceiverId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(super) struct HttpServiceUrl(String);
impl TryFrom<String> for HttpServiceUrl {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let uri = reqwest::Url::parse(&value).map_err(|_| "invalid service URL")?;
        if value.len() > 4096
            || value
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
            || !matches!(uri.scheme(), "http" | "https")
            || uri.host_str().is_none()
            || !uri.username().is_empty()
            || uri.password().is_some()
            || uri.fragment().is_some()
        {
            return Err("invalid service URL");
        }
        Ok(Self(value))
    }
}
impl From<HttpServiceUrl> for String {
    fn from(value: HttpServiceUrl) -> Self {
        value.0
    }
}
impl HttpServiceUrl {
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(super) struct DeviceLoginClientId(String);
impl TryFrom<String> for DeviceLoginClientId {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 256
            || value.bytes().any(|byte| byte.is_ascii_control())
            || value.trim().is_empty()
        {
            return Err("invalid device-login client ID");
        }
        Ok(Self(value))
    }
}
impl From<DeviceLoginClientId> for String {
    fn from(value: DeviceLoginClientId) -> Self {
        value.0
    }
}
impl DeviceLoginClientId {
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(super) struct MachineSessionEndpoint(String);
impl TryFrom<String> for MachineSessionEndpoint {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let uri = reqwest::Url::parse(&value).map_err(|_| "invalid machine endpoint")?;
        if value.len() > 4096
            || value
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
            || !matches!(uri.scheme(), "http" | "https" | "ws" | "wss")
            || uri.host_str().is_none()
            || !uri.username().is_empty()
            || uri.password().is_some()
            || uri.query().is_some()
            || uri.fragment().is_some()
        {
            return Err("invalid machine endpoint");
        }
        Ok(Self(value))
    }
}
impl From<MachineSessionEndpoint> for String {
    fn from(value: MachineSessionEndpoint) -> Self {
        value.0
    }
}
impl MachineSessionEndpoint {
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(super) struct FleetSecretReference(String);
impl TryFrom<String> for FleetSecretReference {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let key = codex_router_secret_store::model::SecretKey::new(&value)
            .map_err(|_| "invalid Fleet secret reference")?;
        codex_router_secret_store::fleet_secret_key::FleetSecretKey::parse(&key)
            .map_err(|_| "invalid Fleet secret reference")?
            .ok_or("invalid Fleet secret reference")?;
        Ok(Self(value))
    }
}
impl From<FleetSecretReference> for String {
    fn from(value: FleetSecretReference) -> Self {
        value.0
    }
}
impl FleetSecretReference {
    pub(super) fn secret_key(
        &self,
    ) -> Result<codex_router_secret_store::model::SecretKey, LoginFileError> {
        codex_router_secret_store::model::SecretKey::new(&self.0)
            .map_err(|_| LoginFileError::InvalidMetadata)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum LoginAccessReference {
    None {},
    Cloudflare {
        client_id: DeviceLoginClientId,
        client_secret_key: FleetSecretReference,
    },
    SelfHosted {
        assertion_key: FleetSecretReference,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FleetLoginConfiguration {
    pub device_authorization_url: HttpServiceUrl,
    pub token_url: HttpServiceUrl,
    pub device_client_id: DeviceLoginClientId,
    pub machine_resource: HttpServiceUrl,
    pub endpoint: MachineSessionEndpoint,
    pub receiver: ReceiverId,
    pub relay: PeerId,
    pub access: LoginAccessReference,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct MachineLoginMetadata {
    pub organization_id: OrganizationId,
    pub enrollment_id: EnrollmentId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum LoginFileError {
    #[error("machine login metadata must be an owner-only regular file in the Router root")]
    UnsafeFile,
    #[error("invalid machine login configuration or public metadata")]
    InvalidMetadata,
    #[error("machine login metadata could not be read or published")]
    Unavailable,
    #[error(
        "machine login metadata was published but directory synchronization could not be confirmed"
    )]
    PublicationUnconfirmed,
}

pub(super) fn read_configuration(
    root: &Path,
) -> Result<Option<FleetLoginConfiguration>, LoginFileError> {
    read_private_optional(root, "login-config.json")
}
pub(super) fn read_metadata(root: &Path) -> Result<Option<MachineLoginMetadata>, LoginFileError> {
    read_private_optional(root, "machine-login.json")
}

fn read_private_optional<T: serde::de::DeserializeOwned>(
    root: &Path,
    name: &str,
) -> Result<Option<T>, LoginFileError> {
    let path = root.join("fleet").join(name);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(LoginFileError::Unavailable),
    };
    validate_private_file(root, &path, &metadata)?;
    let bytes = fs::read(&path).map_err(|_| LoginFileError::Unavailable)?;
    if bytes.len() > 64 * 1024 {
        return Err(LoginFileError::InvalidMetadata);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| LoginFileError::InvalidMetadata)
}

fn validate_private_file(
    root: &Path,
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<(), LoginFileError> {
    let fleet = root.join("fleet");
    let directory = fs::symlink_metadata(&fleet).map_err(|_| LoginFileError::Unavailable)?;
    let owner = rustix::process::geteuid().as_raw();
    if !root.is_absolute()
        || !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != owner
        || !directory.is_dir()
        || directory.permissions().mode() & 0o077 != 0
        || directory.uid() != owner
        || !path
            .canonicalize()
            .map_err(|_| LoginFileError::Unavailable)?
            .starts_with(
                root.canonicalize()
                    .map_err(|_| LoginFileError::Unavailable)?,
            )
    {
        return Err(LoginFileError::UnsafeFile);
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn persist_metadata(
    root: &Path,
    metadata: &MachineLoginMetadata,
) -> Result<(), LoginFileError> {
    persist_metadata_with_sync(root, metadata, |directory| {
        fs::File::open(directory)?.sync_all()
    })
}

pub(super) fn persist_metadata_with_sync(
    root: &Path,
    metadata: &MachineLoginMetadata,
    sync_directory: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<(), LoginFileError> {
    // A damaged existing state is never overwritten by a new login.
    read_metadata(root)?;
    let directory = root.join("fleet");
    match fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(LoginFileError::Unavailable),
    }
    let directory_metadata =
        fs::symlink_metadata(&directory).map_err(|_| LoginFileError::Unavailable)?;
    if !directory_metadata.is_dir()
        || directory_metadata.permissions().mode() & 0o077 != 0
        || directory_metadata.uid() != rustix::process::geteuid().as_raw()
        || !root.is_absolute()
    {
        return Err(LoginFileError::UnsafeFile);
    }
    // The issuer's injected enrollment ID is already a unique UUIDv7. A single
    // Host owns publication, and a crash residue is refused by create_new.
    let temporary = directory.join(format!(
        ".machine-login-{}.tmp",
        metadata.enrollment_id.as_str()
    ));
    publish_metadata(&directory, &temporary, metadata, sync_directory)
}

fn publish_metadata(
    directory: &Path,
    temporary: &Path,
    metadata: &MachineLoginMetadata,
    sync_directory: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<(), LoginFileError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(temporary)
        .map_err(|_| LoginFileError::Unavailable)?;
    let publish = (|| {
        let bytes = serde_json::to_vec(metadata).map_err(std::io::Error::other)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(temporary, directory.join("machine-login.json"))
    })();
    if publish.is_err() {
        let _cleanup = fs::remove_file(temporary);
    }
    publish.map_err(|_: std::io::Error| LoginFileError::Unavailable)?;
    sync_directory(directory).map_err(|_| LoginFileError::PublicationUnconfirmed)
}

#[cfg(test)]
#[path = "login_files_tests.rs"]
mod tests;
