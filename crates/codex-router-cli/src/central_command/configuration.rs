//! The serve root lock and the persisted credential configuration used by mint.
use super::CentralCommandError;
use base64::{Engine, engine::general_purpose::STANDARD};
use codex_router_core::redaction::SecretString;
use collaboration_central_core::AccessPolicy;
use collaboration_central_tokio::SelfHostedIssuer;
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CentralConfiguration {
    pub policy: AccessPolicy,
    pub receiver: fleet_sync_protocol::ReceiverId,
    pub relay: fleet_sync_protocol::PeerId,
}

pub(super) fn lock_root(root: &Path) -> Result<std::fs::File, CentralCommandError> {
    if !root.is_absolute() {
        return Err(CentralCommandError::InvalidInput("--root must be absolute"));
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root)?;
    let metadata = std::fs::symlink_metadata(root)?;
    if !metadata.is_dir()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(CentralCommandError::InvalidInput(
            "central root must be a private directory owned by this account",
        ));
    }
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(root.join("central.lock"))?;
    fs2::FileExt::try_lock_exclusive(&file)
        .map_err(|_| CentralCommandError::InvalidInput("central root is already in use"))?;
    Ok(file)
}

pub(super) fn read(root: &Path) -> Result<CentralConfiguration, CentralCommandError> {
    if !root.is_absolute() {
        return Err(CentralCommandError::InvalidInput("--root must be absolute"));
    }
    let config: CentralConfiguration =
        serde_json::from_slice(&std::fs::read(root.join("central.json"))?)
            .map_err(|_| CentralCommandError::InvalidInput("invalid central configuration"))?;
    config.policy.validate()?;
    Ok(config)
}
pub(super) fn decode_issuer(
    encoded: &SecretString,
    config: &CentralConfiguration,
) -> Result<SelfHostedIssuer, CentralCommandError> {
    let bytes = STANDARD
        .decode(encoded.expose_secret())
        .map_err(|_| CentralCommandError::InvalidInput("invalid central signing key"))?;
    Ok(SelfHostedIssuer::from_pkcs8(&bytes, config.policy.clone())?)
}
