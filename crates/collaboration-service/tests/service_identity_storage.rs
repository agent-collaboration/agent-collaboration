#![allow(clippy::expect_used)]

use collaboration_service::{load_installation_peer, load_service_identity};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

fn private_identity_root() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("identity fixture directory");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private identity fixture permissions");
    assert!(directory.path().is_absolute());
    directory
}

#[test]
fn persistent_identity_survives_restart_and_corruption_never_reinitializes_it() {
    let root = std::path::PathBuf::from(format!("/tmp/service-identity-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap_or_else(|e| panic!("directory: {e}"));
    let first = load_service_identity(&root).unwrap_or_else(|e| panic!("create: {e}"));
    let again = load_service_identity(&root).unwrap_or_else(|e| panic!("read: {e}"));
    assert_eq!(first, again);
    let path = root.join("service-identity.json");
    assert_eq!(
        std::fs::metadata(&path)
            .unwrap_or_else(|e| panic!("metadata: {e}"))
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    std::fs::write(&path, b"broken").unwrap_or_else(|e| panic!("corrupt fixture: {e}"));
    assert!(load_service_identity(&root).is_err());
    assert_eq!(
        std::fs::read(&path).unwrap_or_else(|e| panic!("read: {e}")),
        b"broken"
    );
    std::fs::remove_file(path).unwrap_or_else(|e| panic!("cleanup: {e}"));
    std::fs::remove_dir(root).unwrap_or_else(|e| panic!("cleanup: {e}"));
}

#[test]
fn installation_peer_is_stable_uuidv7_and_preserves_the_existing_service_file() {
    let directory = private_identity_root();
    let path = directory.path().join("service-identity.json");
    std::fs::write(
        &path,
        br#"{"version":1,"serviceId":"00000000-0000-4000-8000-000000000001"}"#,
    )
    .expect("existing UUIDv4 service fixture");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .expect("private service identity fixture");
    let service = load_service_identity(directory.path()).expect("existing service identity");
    let service_bytes =
        std::fs::read(directory.path().join("service-identity.json")).expect("service bytes");
    let first = load_installation_peer(directory.path()).expect("first installation peer");
    assert_eq!(
        load_installation_peer(directory.path()).expect("reopened peer"),
        first
    );
    assert_eq!(first.to_string().as_bytes()[14], b'7');
    assert_eq!(String::from(service.clone()).as_bytes()[14], b'4');
    assert_eq!(
        load_service_identity(directory.path()).expect("same service"),
        service
    );
    assert_eq!(
        std::fs::read(directory.path().join("service-identity.json"))
            .expect("unchanged service bytes"),
        service_bytes
    );
    assert_eq!(
        std::fs::metadata(directory.path().join("installation-peer.json"))
            .expect("peer metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn corrupt_or_non_v7_installation_peer_refuses_without_replacement() {
    for content in [
        b"broken".as_slice(),
        br#"{"version":1,"peerId":"00000000-0000-4000-8000-000000000001"}"#,
        br#"{"version":2,"peerId":"01900000-0000-7000-8000-000000000001"}"#,
        br#"{"version":1,"peerId":"01900000-0000-7000-8000-000000000001","extra":true}"#,
    ] {
        let directory = private_identity_root();
        load_installation_peer(directory.path()).expect("initialize peer");
        let path = directory.path().join("installation-peer.json");
        std::fs::write(&path, content).expect("corrupt peer fixture");
        assert!(load_installation_peer(directory.path()).is_err());
        assert_eq!(
            std::fs::read(path).expect("preserved peer content"),
            content
        );
    }
}

#[test]
fn concurrent_first_starts_publish_one_complete_installation_peer() {
    let directory = private_identity_root();
    let start = std::sync::Barrier::new(8);
    let peers = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(8);
        for _ in 0..8 {
            handles.push(scope.spawn(|| {
                start.wait();
                load_installation_peer(directory.path())
            }));
        }
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .expect("peer creator join")
                    .expect("peer creator")
            })
            .collect::<Vec<_>>()
    });
    assert!(peers.iter().all(|peer| *peer == peers[0]));
    assert_eq!(
        std::fs::read_dir(directory.path())
            .expect("peer inventory")
            .count(),
        1
    );
}

#[test]
fn configured_service_identity_cannot_replace_its_installation_peer() {
    let first_root = private_identity_root();
    let second_root = private_identity_root();
    let first = load_installation_peer(first_root.path()).expect("first installation peer");
    let second = load_installation_peer(second_root.path()).expect("different installation peer");
    assert_ne!(first, second);
    let identity = collaboration_service::ServiceIdentity::new(
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000002",
    )
    .expect("query identity")
    .with_installation_peer(first)
    .expect("configured installation");
    assert_eq!(identity.installation_peer(), Some(first));
    assert!(identity.clone().with_installation_peer(second).is_err());
    assert_eq!(
        identity
            .clone()
            .with_installation_peer(first)
            .expect("same identity replay")
            .installation_peer(),
        Some(first)
    );
    assert_eq!(identity.installation_peer(), Some(first));
}
