//! One declared Host's existing singleton lock spans every offline cutover effect.
use codex_router_host::{HostCoordinationPaths, HostInstanceLock, InstanceAcquireError};
use collaboration_protocol::UuidIdentity;
use fleet_sync_protocol::PeerId;
use std::os::unix::fs::MetadataExt;
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

impl OfflineHost {
    pub fn acquire(
        root: &Path,
        expected_service: &UuidIdentity,
        expected_peer: PeerId,
        home: &Path,
    ) -> Result<Self, OfflineHostError> {
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
        // Identities are immutable under the same Host authority. Establish
        // presence before calling the owning loader, whose missing-file path
        // creates a new identity and is forbidden during cutover.
        match std::fs::symlink_metadata(collaboration.join("installation-peer.json")) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(OfflineHostError::MissingPeerIdentity);
            }
            Err(error) => return Err(error.into()),
        }
        let authority = HostInstanceLock::acquire(&HostCoordinationPaths::for_router_root(&root))?;
        let peer = collaboration_runtime::load_installation_peer(&collaboration)?;
        if peer != expected_peer {
            return Err(OfflineHostError::PeerMismatch);
        }
        Ok(Self {
            root,
            collaboration,
            service,
            peer,
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
