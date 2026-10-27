//! Atomic creation of stable logical Host identity; corruption never creates a replacement.
use collaboration_protocol::UuidIdentity;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdentityRecord {
    version: u8,
    service_id: UuidIdentity,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PeerIdentityRecord {
    version: u8,
    peer_id: fleet_sync_protocol::PeerId,
}

/// Reads the existing Host identity without creating a directory or identity file.
pub fn read_service_identity(directory: &Path) -> io::Result<Option<UuidIdentity>> {
    match read_identity(&directory.join("service-identity.json")) {
        Ok(identity) => Ok(Some(identity)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Generates an independent epoch/identity from OS randomness.
pub fn new_service_uuid() -> io::Result<UuidIdentity> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| io::Error::other("identity randomness unavailable"))?;
    if let Some(value) = bytes.get_mut(6) {
        *value = (*value & 0x0f) | 0x40;
    }
    if let Some(value) = bytes.get_mut(8) {
        *value = (*value & 0x3f) | 0x80;
    }
    let mut encoded = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            encoded.push('-');
        }
        use std::fmt::Write as _;
        write!(encoded, "{byte:02x}").map_err(|_| io::Error::other("identity encoding"))?;
    }
    UuidIdentity::try_from(encoded).map_err(io::Error::other)
}

pub fn load_service_identity(directory: &Path) -> io::Result<UuidIdentity> {
    let metadata = std::fs::symlink_metadata(directory)?;
    if !directory.is_absolute() || !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "identity directory must be private and absolute",
        ));
    }
    let path = directory.join("service-identity.json");
    match read_identity(&path) {
        Ok(identity) => return Ok(identity),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let identity = new_service_uuid()?;
    let suffix: String = new_service_uuid()?.into();
    let temporary = directory.join(format!(".service-identity-{suffix}.tmp"));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        let bytes = serde_json::to_vec(&IdentityRecord {
            version: 1,
            service_id: identity.clone(),
        })
        .map_err(io::Error::other)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        // Link installs the complete file without ever replacing a concurrent creator.
        match std::fs::hard_link(&temporary, &path) {
            Ok(()) => {
                File::open(directory)?.sync_all()?;
                Ok(identity)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => read_identity(&path),
            Err(error) => Err(error),
        }
    })();
    let cleanup = std::fs::remove_file(&temporary);
    match result {
        Ok(identity) => {
            cleanup?;
            Ok(identity)
        }
        Err(error) => Err(error),
    }
}
fn read_identity(path: &Path) -> io::Result<UuidIdentity> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 || metadata.len() > 1024 {
        return Err(io::Error::other("invalid identity file"));
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(1025).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 {
        return Err(io::Error::other("identity file too large"));
    }
    let record: IdentityRecord =
        serde_json::from_slice(&bytes).map_err(|_| io::Error::other("invalid identity content"))?;
    if record.version != 1 {
        return Err(io::Error::other("unsupported identity version"));
    }
    Ok(record.service_id)
}

/// The installation's UUIDv7 peer is independent of its existing service UUID and enrollment.
pub fn load_installation_peer(directory: &Path) -> io::Result<fleet_sync_protocol::PeerId> {
    let metadata = std::fs::symlink_metadata(directory)?;
    if !directory.is_absolute() || !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "identity directory must be private and absolute",
        ));
    }
    let path = directory.join("installation-peer.json");
    match read_peer_identity(&path) {
        Ok(peer) => return Ok(peer),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let peer = fleet_sync_protocol::PeerId::new(uuid::Uuid::now_v7()).map_err(io::Error::other)?;
    let temporary = directory.join(format!(".installation-peer-{}.tmp", uuid::Uuid::now_v7()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        let bytes = serde_json::to_vec(&PeerIdentityRecord {
            version: 1,
            peer_id: peer,
        })
        .map_err(io::Error::other)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        match std::fs::hard_link(&temporary, &path) {
            Ok(()) => {
                File::open(directory)?.sync_all()?;
                Ok(peer)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => read_peer_identity(&path),
            Err(error) => Err(error),
        }
    })();
    let cleanup = std::fs::remove_file(&temporary);
    match result {
        Ok(peer) => {
            cleanup?;
            Ok(peer)
        }
        Err(error) => Err(error),
    }
}

fn read_peer_identity(path: &Path) -> io::Result<fleet_sync_protocol::PeerId> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 || metadata.len() > 1024 {
        return Err(io::Error::other("invalid installation peer file"));
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(1025).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 {
        return Err(io::Error::other("installation peer file too large"));
    }
    let record: PeerIdentityRecord = serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::other("invalid installation peer content"))?;
    if record.version != 1 {
        return Err(io::Error::other("unsupported installation peer version"));
    }
    Ok(record.peer_id)
}

#[cfg(test)]
mod read_only_identity_tests {
    use super::*;

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Contract asserts; setup may fail."
    )]
    fn read_existing_identity_never_creates_or_replaces_files() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        assert_eq!(read_service_identity(directory.path())?, None);
        assert_eq!(std::fs::read_dir(directory.path())?.count(), 0);
        let path = directory.path().join("service-identity.json");
        let bytes = br#"{"version":1,"serviceId":"550e8400-e29b-41d4-a716-446655440000"}"#;
        std::fs::write(&path, bytes)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let modified = path.metadata()?.modified()?;
        assert_eq!(
            String::from(
                read_service_identity(directory.path())?
                    .ok_or_else(|| io::Error::other("identity missing"))?
            ),
            "550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(std::fs::read(&path)?, bytes);
        assert_eq!(modified, path.metadata()?.modified()?);
        std::fs::write(&path, b"invalid")?;
        assert!(read_service_identity(directory.path()).is_err());
        assert_eq!(std::fs::read(&path)?, b"invalid");
        Ok(())
    }
}
