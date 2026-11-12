use collaboration_runtime::load_service_identity;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
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
fn fresh_root_mints_a_uuidv7_service_identity_file() {
    let directory = private_identity_root();

    let minted = load_service_identity(directory.path()).expect("fresh service identity");

    let stored: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("service-identity.json")).expect("identity file"),
    )
    .expect("identity JSON");
    let stored_id = stored["serviceId"].as_str().expect("serviceId");
    assert_eq!(stored_id, String::from(minted));
    assert_eq!(
        uuid::Uuid::parse_str(stored_id)
            .expect("stored service id")
            .get_version_num(),
        7
    );
}
