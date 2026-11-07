use super::{OfflineHost, OfflineHostError};
use crate::cutover_host_fixture::CutoverHostFixture;
use codex_router_host::{HostCoordinationPaths, HostInstanceLock, InstanceAcquireError};
use collaboration_protocol::UuidIdentity;
use fleet_protocol::PeerId;
use std::os::unix::fs::PermissionsExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Production opt-in never admits a standing debug root."
)]
fn production_mode_only_admits_the_exact_home_default_root() -> TestResult {
    for name in [".codex", ".codex-router-debug", "isolated-staging"] {
        let fixture = CutoverHostFixture::new(Some(name))?;
        let result = OfflineHost::inspect_mode(
            &fixture.root,
            &fixture.service,
            fixture.peer,
            &fixture.home,
            super::RootMode::Production,
        );
        assert!(result.is_err());
        assert!(
            !HostCoordinationPaths::for_router_root(&fixture.root)
                .instance_lock()
                .exists()
        );
    }
    let fixture = CutoverHostFixture::new(Some(".codex-router"))?;
    let inspection = OfflineHost::inspect_mode(
        &fixture.root,
        &fixture.service,
        fixture.peer,
        &fixture.home,
        super::RootMode::Production,
    )?;
    assert_eq!(
        inspection.router_root(),
        std::fs::canonicalize(&fixture.root)?
    );
    assert!(
        !HostCoordinationPaths::for_router_root(&fixture.root)
            .instance_lock()
            .exists()
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Read-only inspection cannot create even an inert Host lock file."
)]
fn read_only_inspection_of_a_stopped_host_creates_no_lock_artifact() -> TestResult {
    // Arrange an existing private identity with no Host coordination artifact.
    let fixture = CutoverHostFixture::new(None)?;
    let paths = HostCoordinationPaths::for_router_root(&fixture.root);
    assert!(!paths.instance_lock().exists());
    // Act through the same pre-stop inspection boundary.
    OfflineHost::inspect(&fixture.root, &fixture.service, fixture.peer, &fixture.home)?;
    // Assert a read creates no authority or filesystem state.
    assert!(!paths.instance_lock().exists());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Pre-stop inspection must keep the live Host's lock and identities untouched."
)]
fn read_only_inspection_validates_a_live_host_without_acquiring_its_lock() -> TestResult {
    // Arrange the same lock held by an already running declared Host.
    let fixture = CutoverHostFixture::new(None)?;
    let paths = HostCoordinationPaths::for_router_root(&fixture.root);
    let host = HostInstanceLock::acquire(&paths)?;
    let peer_path = fixture.identity_path("installation-peer.json");
    let before = std::fs::read(&peer_path)?;
    // Act through the pre-stop boundary the TypeScript orchestrator will use.
    let inspected =
        OfflineHost::inspect(&fixture.root, &fixture.service, fixture.peer, &fixture.home)?;
    // Assert the exact declared identity and that the running authority remains held.
    assert_eq!(
        inspected.router_root(),
        std::fs::canonicalize(&fixture.root)?
    );
    assert_eq!(inspected.service_id(), &fixture.service);
    assert_eq!(inspected.peer(), fixture.peer);
    assert_eq!(std::fs::read(peer_path)?, before);
    assert!(matches!(
        HostInstanceLock::acquire(&paths),
        Err(InstanceAcquireError::AlreadyRunning)
    ));
    drop(host);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The real singleton lock must span the offline scope."
)]
fn offline_scope_holds_the_existing_host_lock_until_drop() -> TestResult {
    // Arrange / Act: acquire the declared Host's actual offline authority.
    let fixture = CutoverHostFixture::new(None)?;
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
    let fixture = CutoverHostFixture::new(None)?;
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
        let fixture = CutoverHostFixture::new(Some(name))?;
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
    let fixture = CutoverHostFixture::new(None)?;
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
    let fixture = CutoverHostFixture::new(None)?;
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
    let fixture = CutoverHostFixture::new(None)?;
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
    let fixture = CutoverHostFixture::new(None)?;
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
    let fixture = CutoverHostFixture::new(None)?;
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
