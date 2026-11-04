//! One declared Host's existing singleton lock spans every offline cutover effect.
use codex_router_host::{HostCoordinationPaths, HostInstanceLock, InstanceAcquireError};
use collaboration_protocol::UuidIdentity;
use fleet_sync_protocol::PeerId;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum OfflineHostError {
    #[error("the declared Router root must be absolute")]
    RelativeRoot,
    #[error("production and standing debug Router roots are not cutover targets")]
    ProtectedRoot,
    #[error("the declared Router root must be an owner-private directory")]
    UnsafeRoot,
    #[error("the declared Host has no existing service identity")]
    MissingServiceIdentity,
    #[error("the declared Host has no existing installation peer")]
    MissingPeerIdentity,
    #[error("the declared Host service differs from the preserved source inventory")]
    ServiceMismatch,
    #[error("the declared Host peer differs from the preserved source inventory")]
    PeerMismatch,
    #[error("cannot inspect the declared offline Host")]
    Io(#[from] std::io::Error),
    #[error("cannot acquire the declared Host's offline lock")]
    Lock(#[from] InstanceAcquireError),
}

pub struct OfflineHost {
    root: PathBuf,
    collaboration: PathBuf,
    service: UuidIdentity,
    peer: PeerId,
    _authority: HostInstanceLock,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostInspection {
    #[serde(rename = "routerRoot")]
    root: PathBuf,
    #[serde(rename = "serviceId")]
    service: UuidIdentity,
    #[serde(rename = "peerId")]
    peer: PeerId,
}

impl HostInspection {
    pub fn router_root(&self) -> &Path {
        &self.root
    }
    pub fn service_id(&self) -> &UuidIdentity {
        &self.service
    }
    pub const fn peer(&self) -> PeerId {
        self.peer
    }
}

impl OfflineHost {
    pub fn inspect(
        root: &Path,
        expected_service: &UuidIdentity,
        expected_peer: PeerId,
        home: &Path,
    ) -> Result<HostInspection, OfflineHostError> {
        if !root.is_absolute() {
            return Err(OfflineHostError::RelativeRoot);
        }
        if protected_root(root, home) {
            return Err(OfflineHostError::ProtectedRoot);
        }
        require_private_directory(root)?;
        let root = std::fs::canonicalize(root)?;
        let home = std::fs::canonicalize(home)?;
        if protected_root(&root, &home) {
            return Err(OfflineHostError::ProtectedRoot);
        }
        let collaboration = codex_router_host::router_collaboration_directory(&root);
        require_private_directory(&collaboration)?;
        let service = collaboration_runtime::read_service_identity(&collaboration)?
            .ok_or(OfflineHostError::MissingServiceIdentity)?;
        if &service != expected_service {
            return Err(OfflineHostError::ServiceMismatch);
        }
        let peer = read_existing_peer(&collaboration)?;
        if peer != expected_peer {
            return Err(OfflineHostError::PeerMismatch);
        }
        Ok(HostInspection {
            root,
            service,
            peer,
        })
    }

    pub fn acquire(
        root: &Path,
        expected_service: &UuidIdentity,
        expected_peer: PeerId,
        home: &Path,
    ) -> Result<Self, OfflineHostError> {
        let declared = Self::inspect(root, expected_service, expected_peer, home)?;
        let authority =
            HostInstanceLock::acquire(&HostCoordinationPaths::for_router_root(&declared.root))?;
        // Re-read immutable identities under the acquired authority before any export.
        let declared = Self::inspect(&declared.root, expected_service, expected_peer, home)?;
        Ok(Self {
            collaboration: codex_router_host::router_collaboration_directory(&declared.root),
            root: declared.root,
            service: declared.service,
            peer: declared.peer,
            _authority: authority,
        })
    }

    pub fn router_root(&self) -> &Path {
        &self.root
    }
    pub fn collaboration_directory(&self) -> &Path {
        &self.collaboration
    }
    pub fn service_id(&self) -> &UuidIdentity {
        &self.service
    }
    pub const fn peer(&self) -> PeerId {
        self.peer
    }
}

// A read-only adapter for the existing version-one file. The runtime's public
// loader creates a missing identity, which is forbidden before Host stop too.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExistingPeerIdentity {
    version: u8,
    peer_id: PeerId,
}

fn read_existing_peer(directory: &Path) -> Result<PeerId, OfflineHostError> {
    let path = directory.join("installation-peer.json");
    let before = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(OfflineHostError::MissingPeerIdentity);
        }
        Err(error) => return Err(error.into()),
    };
    if !before.is_file()
        || before.uid() != nix::unistd::geteuid().as_raw()
        || before.mode() & 0o077 != 0
        || before.len() > 1024
    {
        return Err(OfflineHostError::UnsafeRoot);
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(&path)?;
    let opened = file.metadata()?;
    if opened.dev() != before.dev() || opened.ino() != before.ino() {
        return Err(OfflineHostError::UnsafeRoot);
    }
    let mut bytes = Vec::new();
    file.take(1025).read_to_end(&mut bytes)?;
    let invalid = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid existing installation peer",
        )
    };
    if bytes.len() > 1024 {
        return Err(invalid().into());
    }
    let record: ExistingPeerIdentity = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if record.version != 1 {
        return Err(invalid().into());
    }
    Ok(record.peer_id)
}

fn protected_root(root: &Path, home: &Path) -> bool {
    [".codex", ".codex-router", ".codex-router-debug"]
        .iter()
        .any(|name| root.starts_with(home.join(name)))
}

fn require_private_directory(path: &Path) -> Result<(), OfflineHostError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.mode() & 0o077 != 0
        || metadata.uid() != nix::unistd::geteuid().as_raw()
    {
        return Err(OfflineHostError::UnsafeRoot);
    }
    Ok(())
}

#[cfg(test)]
#[path = "offline_host_test.rs"]
mod tests;
