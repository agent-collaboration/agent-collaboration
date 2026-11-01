//! The new login reads the old document's Access reference without reviving its machine identity.
use super::*;
use serde_json::json;

const ACCESS_KEY: &str = "fleet_access_client_secret.019f0000-0000-7000-8000-000000000001.019f0000-0000-7000-8000-000000000002";

fn write_access_file(root: &Path) -> std::path::PathBuf {
    let path = crate::router_fleet_enrollment_file(root);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "access":{"clientId":"fixture-access","clientSecretKey":ACCESS_KEY},
            "credentialKey":"retired-and-no-longer-a-valid-key",
            "endpoint":"retired-and-no-longer-a-valid-url",
            "peer":"retired-and-no-longer-a-valid-peer"
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    path
}

#[test]
fn fleet_machine_login_access_reader_ignores_retired_machine_fields_and_never_opens_a_store() {
    let root = tempfile::tempdir().unwrap();
    let path = write_access_file(root.path());
    let original = std::fs::read(&path).unwrap();

    let Some(FleetAccess::Cloudflare(reference)) = load_router_fleet_access(root.path()).unwrap()
    else {
        panic!("Access reference missing");
    };

    assert_eq!(reference.client_id, "fixture-access");
    assert_eq!(reference.client_secret_key, ACCESS_KEY);
    assert!(!root.path().join("secrets").exists());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(
        read_fleet_enrollment_document(&path, root.path()).is_err(),
        "the unchanged step-one reader still validates its entire contract"
    );
}

#[test]
fn fleet_machine_login_access_reader_preserves_private_file_and_closed_access_boundaries() {
    let root = tempfile::tempdir().unwrap();
    assert!(load_router_fleet_access(root.path()).unwrap().is_none());
    let path = write_access_file(root.path());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(load_router_fleet_access(root.path()).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    for access in [
        json!({"clientId":"fixture-access","clientSecret":"inline-secret-canary"}),
        json!({"clientId":"fixture-access","clientSecretKey":ACCESS_KEY,"unexpected":true}),
    ] {
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({"access":access})).unwrap(),
        )
        .unwrap();
        let error = load_router_fleet_access(root.path()).err().unwrap();
        assert!(!error.to_string().contains("inline-secret-canary"));
    }
    let target = root.path().join("private-access.json");
    std::fs::rename(&path, &target).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(load_router_fleet_access(root.path()).is_err());
}
