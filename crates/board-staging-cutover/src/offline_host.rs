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
    #[error("--production requires the exact home-default production Router root")]
    ProductionRootRequired,
    #[error("the explicit production target differs from the run plan")]
    TargetMismatch,
    #[error("production confirmation differs from the current dry-run plan; run --dry-run again")]
    ConfirmationMismatch,
    #[error("cannot read the current cutover plan without side effects")]
    PlanUnavailable,
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

#[derive(Clone, Copy, Default)]
pub(crate) enum RootMode {
    #[default]
    Isolated,
    Production,
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
        Self::inspect_mode(
            root,
            expected_service,
            expected_peer,
            home,
            RootMode::Isolated,
        )
    }

    pub(crate) fn inspect_mode(
        root: &Path,
        expected_service: &UuidIdentity,
        expected_peer: PeerId,
        home: &Path,
        mode: RootMode,
    ) -> Result<HostInspection, OfflineHostError> {
        if !root.is_absolute() {
            return Err(OfflineHostError::RelativeRoot);
        }
        if matches!(mode, RootMode::Isolated) {
            require_allowed_root(root, home, mode)?;
        }
        require_private_directory(root)?;
        let root = std::fs::canonicalize(root)?;
        let home = std::fs::canonicalize(home)?;
        require_allowed_root(&root, &home, mode)?;
        let collaboration = codex_router_host::router_collaboration_directory(&root);
        require_private_directory(&collaboration)?;
        let service = collaboration_runtime::read_service_identity(&collaboration)?
            .ok_or(OfflineHostError::MissingServiceIdentity)?;
        if &service != expected_service {
            return Err(OfflineHostError::ServiceMismatch);
        }
        let peer = collaboration_runtime::read_installation_peer(&collaboration)?
            .ok_or(OfflineHostError::MissingPeerIdentity)?;
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
        Self::acquire_mode(
            root,
            expected_service,
            expected_peer,
            home,
            RootMode::Isolated,
        )
    }

    pub(crate) fn acquire_mode(
        root: &Path,
        expected_service: &UuidIdentity,
        expected_peer: PeerId,
        home: &Path,
        mode: RootMode,
    ) -> Result<Self, OfflineHostError> {
        let declared = Self::inspect_mode(root, expected_service, expected_peer, home, mode)?;
        let authority =
            HostInstanceLock::acquire(&HostCoordinationPaths::for_router_root(&declared.root))?;
        // Re-read immutable identities under the acquired authority before any export.
        let declared =
            Self::inspect_mode(&declared.root, expected_service, expected_peer, home, mode)?;
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

fn require_allowed_root(root: &Path, home: &Path, mode: RootMode) -> Result<(), OfflineHostError> {
    if [".codex", ".codex-router-debug"]
        .iter()
        .any(|name| root.starts_with(home.join(name)))
    {
        return Err(OfflineHostError::ProtectedRoot);
    }
    let production = home.join(".codex-router");
    match mode {
        RootMode::Isolated if root.starts_with(&production) => Err(OfflineHostError::ProtectedRoot),
        RootMode::Production if root != production => Err(OfflineHostError::ProductionRootRequired),
        RootMode::Isolated | RootMode::Production => Ok(()),
    }
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
