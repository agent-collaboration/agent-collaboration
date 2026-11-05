//! Private Host roots and existing identities shared by cutover component proofs.
use crate::{OfflineHost, OfflineHostError};
use collaboration_protocol::UuidIdentity;
use fleet_sync_protocol::PeerId;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

pub(crate) struct CutoverHostFixture {
    pub(crate) _area: tempfile::TempDir,
    pub(crate) root: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) service: UuidIdentity,
    pub(crate) peer: PeerId,
}

impl CutoverHostFixture {
    pub(crate) fn new(protected: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        match protected {
            None => Self::new_in(&std::env::temp_dir()),
            Some(name) => Self::with_area(tempfile::tempdir()?, Some(name)),
        }
    }

    pub(crate) fn new_in(parent: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_area(
            tempfile::Builder::new().prefix("h-").tempdir_in(parent)?,
            None,
        )
    }

    fn with_area(
        area: tempfile::TempDir,
        protected: Option<&str>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let home = area.path().join("home");
        std::fs::create_dir(&home)?;
        let root = protected.map_or_else(|| area.path().join("machine"), |name| home.join(name));
        std::fs::create_dir(&root)?;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        let collaboration = codex_router_host::router_collaboration_directory(&root);
        std::fs::create_dir(&collaboration)?;
        std::fs::set_permissions(&collaboration, std::fs::Permissions::from_mode(0o700))?;
        let service = uuid::Uuid::now_v7().to_string().try_into()?;
        let peer = PeerId::new(uuid::Uuid::now_v7())?;
        let fixture = Self {
            _area: area,
            root,
            home,
            service,
            peer,
        };
        for (name, value) in [
            (
                "service-identity.json",
                serde_json::json!({"version":1,"serviceId":fixture.service}),
            ),
            (
                "installation-peer.json",
                serde_json::json!({"version":1,"peerId":peer}),
            ),
        ] {
            let path = collaboration.join(name);
            std::fs::write(&path, serde_json::to_vec(&value)?)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(fixture)
    }

    pub(crate) fn acquire(&self) -> Result<OfflineHost, OfflineHostError> {
        OfflineHost::acquire(&self.root, &self.service, self.peer, &self.home)
    }

    pub(crate) fn identity_path(&self, name: &str) -> PathBuf {
        codex_router_host::router_collaboration_directory(&self.root).join(name)
    }
}
