use super::{OfflineHost, OfflineHostError};
use codex_router_host::{HostCoordinationPaths, HostInstanceLock, InstanceAcquireError};
use collaboration_protocol::UuidIdentity;
use fleet_sync_protocol::PeerId;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    _area: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    service: UuidIdentity,
    peer: PeerId,
}

impl Fixture {
    fn new(protected: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        let area = tempfile::tempdir()?;
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

    fn acquire(&self) -> Result<OfflineHost, OfflineHostError> {
        OfflineHost::acquire(&self.root, &self.service, self.peer, &self.home)
    }

    fn identity_path(&self, name: &str) -> PathBuf {
        codex_router_host::router_collaboration_directory(&self.root).join(name)
    }
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The real singleton lock must span the offline scope."
)]
fn offline_scope_holds_the_existing_host_lock_until_drop() -> TestResult {
    // Arrange / Act: acquire the declared Host's actual offline authority.
    let fixture = Fixture::new(None)?;
    let offline = fixture.acquire()?;
    // Assert: same public Host acquisition is excluded until the owner drops.
    assert_eq!(offline.router_root(), std::fs::canonicalize(&fixture.root)?);
    assert_eq!(offline.service_id(), &fixture.service);
    assert_eq!(offline.peer(), fixture.peer);
    let paths = HostCoordinationPaths::for_router_root(&fixture.root);
    assert!(matches!(
        HostInstanceLock::acquire(&paths),
        Err(InstanceAcquireError::AlreadyRunning)
    ));
    drop(offline);
    let host = HostInstanceLock::acquire(&paths)?;
    drop(host);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A live Host's same lock must exclude the helper."
)]
fn existing_host_lock_prevents_offline_access() -> TestResult {
    // Arrange: the actual Host lock is already held.
    let fixture = Fixture::new(None)?;
    let host = HostInstanceLock::acquire(&HostCoordinationPaths::for_router_root(&fixture.root))?;
    // Act / Assert: no second offline capability is admitted.
    assert!(matches!(
        fixture.acquire(),
        Err(OfflineHostError::Lock(InstanceAcquireError::AlreadyRunning))
    ));
    drop(host);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Protected roots must refuse before any lock artifact."
)]
fn production_and_standing_debug_roots_are_refused_without_side_effects() -> TestResult {
    for name in [".codex", ".codex-router", ".codex-router-debug"] {
        // Arrange: artificial home roots, never the actual user's production directory.
        let fixture = Fixture::new(Some(name))?;
        // Act / Assert.
        assert!(matches!(
            fixture.acquire(),
            Err(OfflineHostError::ProtectedRoot)
        ));
        assert!(
            !HostCoordinationPaths::for_router_root(&fixture.root)
                .instance_lock()
                .exists()
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A shared or symlink root is not the declared private Host."
)]
fn public_and_symlink_roots_are_refused() -> TestResult {
    // Arrange / Act / Assert: a public directory is rejected.
    let fixture = Fixture::new(None)?;
    std::fs::set_permissions(&fixture.root, std::fs::Permissions::from_mode(0o755))?;
    assert!(matches!(
        fixture.acquire(),
        Err(OfflineHostError::UnsafeRoot)
    ));
    std::fs::set_permissions(&fixture.root, std::fs::Permissions::from_mode(0o700))?;
    // A direct symlink must not silently select another Host.
    let link = fixture.home.join("linked-machine");
    std::os::unix::fs::symlink(&fixture.root, &link)?;
    assert!(matches!(
        OfflineHost::acquire(&link, &fixture.service, fixture.peer, &fixture.home),
        Err(OfflineHostError::UnsafeRoot)
    ));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The preserved service identity must match without replacement."
)]
fn source_service_mismatch_keeps_the_original_identity() -> TestResult {
    // Arrange.
    let fixture = Fixture::new(None)?;
    let path = fixture.identity_path("service-identity.json");
    let before = std::fs::read(&path)?;
    let other: UuidIdentity = uuid::Uuid::now_v7().to_string().try_into()?;
    // Act / Assert.
    assert!(matches!(
        OfflineHost::acquire(&fixture.root, &other, fixture.peer, &fixture.home),
        Err(OfflineHostError::ServiceMismatch)
    ));
    assert_eq!(std::fs::read(path)?, before);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The preserved physical installation peer must match."
)]
fn source_peer_mismatch_keeps_the_original_identity() -> TestResult {
    // Arrange.
    let fixture = Fixture::new(None)?;
    let path = fixture.identity_path("installation-peer.json");
    let before = std::fs::read(&path)?;
    let other = PeerId::new(uuid::Uuid::now_v7())?;
    // Act / Assert.
    assert!(matches!(
        OfflineHost::acquire(&fixture.root, &fixture.service, other, &fixture.home),
        Err(OfflineHostError::PeerMismatch)
    ));
    assert_eq!(std::fs::read(path)?, before);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Cutover must never create a missing installation identity."
)]
fn missing_peer_identity_is_not_created() -> TestResult {
    // Arrange: retain the service identity, remove only this fixture's peer file.
    let fixture = Fixture::new(None)?;
    let path = fixture.identity_path("installation-peer.json");
    std::fs::remove_file(&path)?;
    // Act / Assert.
    assert!(matches!(
        fixture.acquire(),
        Err(OfflineHostError::MissingPeerIdentity)
    ));
    assert!(!path.exists());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Cutover reads an existing service identity and never creates one."
)]
fn missing_service_identity_is_not_created() -> TestResult {
    // Arrange: retain the peer identity, remove only this fixture's service file.
    let fixture = Fixture::new(None)?;
    let path = fixture.identity_path("service-identity.json");
    std::fs::remove_file(&path)?;
    // Act / Assert: the read-only identity boundary cannot manufacture a replacement.
    assert!(matches!(
        fixture.acquire(),
        Err(OfflineHostError::MissingServiceIdentity)
    ));
    assert!(!path.exists());
    Ok(())
}
