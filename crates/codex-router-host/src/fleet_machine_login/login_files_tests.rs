use super::*;
use serde_json::{Value, json};
use std::path::PathBuf;

#[test]
fn fleet_machine_login_rename_then_failed_directory_sync_preserves_published_public_metadata() {
    let root = tempfile::tempdir().unwrap();
    write_configuration(root.path(), &fixture_configuration());
    let metadata: MachineLoginMetadata = serde_json::from_value(json!({
        "organizationId":"019f0000-0000-7000-8000-000000000021",
        "enrollmentId":"019f0000-0000-7000-8000-000000000022"
    }))
    .unwrap();
    let result = persist_metadata_with_sync(root.path(), &metadata, |directory| {
        assert_eq!(read_metadata(root.path()).unwrap(), Some(metadata.clone()));
        assert_eq!(directory, root.path().join("fleet"));
        Err(std::io::Error::other("injected directory sync failure"))
    });
    assert_eq!(result, Err(LoginFileError::PublicationUnconfirmed));
    assert_eq!(read_metadata(root.path()).unwrap(), Some(metadata));
}

fn fixture_configuration() -> Value {
    json!({
        "deviceAuthorizationUrl":"https://sign-in.example.invalid/device/code",
        "tokenUrl":"https://sign-in.example.invalid/oauth2/token",
        "deviceClientId":"fixture-device-login",
        "machineResource":"https://central.example.invalid/machine",
        "endpoint":"wss://central.example.invalid/v1/session",
        "receiver":"019f0000-0000-7000-8000-000000000011",
        "relay":"019f0000-0000-7000-8000-000000000012",
        "access":{"kind":"none"}
    })
}

fn write_configuration(root: &Path, value: &Value) -> PathBuf {
    let directory = root.join("fleet");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    let path = directory.join("login-config.json");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    path
}

#[test]
fn fleet_machine_login_configuration_is_missing_or_validated_without_writes() {
    let root = tempfile::tempdir().unwrap();
    assert!(read_configuration(root.path()).unwrap().is_none());
    assert!(!root.path().join("fleet").exists());
    let path = write_configuration(root.path(), &fixture_configuration());
    let before = fs::read(&path).unwrap();
    let configuration = read_configuration(root.path()).unwrap().unwrap();
    assert_eq!(
        configuration.device_client_id.as_str(),
        "fixture-device-login"
    );
    assert_eq!(
        configuration.endpoint.as_str(),
        "wss://central.example.invalid/v1/session"
    );
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn fleet_machine_login_configuration_rejects_unsafe_files_unknown_fields_and_invalid_values() {
    for (field, invalid) in [
        ("tokenUrl", json!("file:///tmp/token")),
        ("deviceClientId", json!("")),
        (
            "endpoint",
            json!("wss://central.example.invalid/v1/session?token=secret"),
        ),
        ("receiver", json!("00000000-0000-4000-8000-000000000001")),
        ("relay", json!("invalid")),
        ("unexpected", json!(true)),
        (
            "access",
            json!({"kind":"cloudflare","clientId":"fixture","clientSecretKey":"plaintext-secret"}),
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut value = fixture_configuration();
        value[field] = invalid;
        write_configuration(root.path(), &value);
        assert!(matches!(
            read_configuration(root.path()),
            Err(LoginFileError::InvalidMetadata)
        ));
    }
    let root = tempfile::tempdir().unwrap();
    let path = write_configuration(root.path(), &fixture_configuration());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(matches!(
        read_configuration(root.path()),
        Err(LoginFileError::UnsafeFile)
    ));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(root.path().join("fleet"), fs::Permissions::from_mode(0o750)).unwrap();
    assert!(matches!(
        read_configuration(root.path()),
        Err(LoginFileError::UnsafeFile)
    ));
}

#[test]
fn fleet_machine_login_public_metadata_is_atomic_private_and_contains_no_credentials() {
    let root = tempfile::tempdir().unwrap();
    let config_path = write_configuration(root.path(), &fixture_configuration());
    let config_before = fs::read(&config_path).unwrap();
    let metadata = MachineLoginMetadata {
        organization_id: "019f0000-0000-7000-8000-000000000021"
            .to_owned()
            .try_into()
            .unwrap(),
        enrollment_id: "019f0000-0000-7000-8000-000000000022"
            .to_owned()
            .try_into()
            .unwrap(),
    };
    persist_metadata(root.path(), &metadata).unwrap();
    assert_eq!(read_metadata(root.path()).unwrap(), Some(metadata.clone()));
    let state_path = root.path().join("fleet/machine-login.json");
    assert_eq!(
        fs::metadata(&state_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let wire: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(
        wire,
        json!({"organizationId":metadata.organization_id,"enrollmentId":metadata.enrollment_id})
    );
    assert_eq!(fs::read(config_path).unwrap(), config_before);
    for path in [root.path().join("fleet/login-config.json"), state_path] {
        let bytes = fs::read(path).unwrap();
        for forbidden in [
            "f11-refresh-canary",
            "f11-access-canary",
            "f11-device-code-canary",
            "f11-client-secret-canary",
        ] {
            assert!(!String::from_utf8_lossy(&bytes).contains(forbidden));
        }
    }
    assert_eq!(fs::read_dir(root.path().join("fleet")).unwrap().count(), 2);
}

#[test]
fn fleet_machine_login_damaged_public_metadata_is_never_overwritten() {
    let root = tempfile::tempdir().unwrap();
    write_configuration(root.path(), &fixture_configuration());
    let path = root.path().join("fleet/machine-login.json");
    fs::write(&path, b"damaged").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let metadata = MachineLoginMetadata {
        organization_id: "019f0000-0000-7000-8000-000000000021"
            .to_owned()
            .try_into()
            .unwrap(),
        enrollment_id: "019f0000-0000-7000-8000-000000000022"
            .to_owned()
            .try_into()
            .unwrap(),
    };
    assert_eq!(
        persist_metadata(root.path(), &metadata),
        Err(LoginFileError::InvalidMetadata)
    );
    assert_eq!(fs::read(path).unwrap(), b"damaged");
}

#[test]
fn fleet_machine_login_publication_never_removes_a_temporary_file_it_did_not_create() {
    let root = tempfile::tempdir().unwrap();
    write_configuration(root.path(), &fixture_configuration());
    let metadata = MachineLoginMetadata {
        organization_id: "019f0000-0000-7000-8000-000000000021"
            .to_owned()
            .try_into()
            .unwrap(),
        enrollment_id: "019f0000-0000-7000-8000-000000000022"
            .to_owned()
            .try_into()
            .unwrap(),
    };
    let previous = root
        .path()
        .join("fleet/.machine-login-019f0000-0000-7000-8000-000000000022.tmp");
    fs::write(&previous, b"previous-attempt").unwrap();
    assert_eq!(
        persist_metadata(root.path(), &metadata),
        Err(LoginFileError::Unavailable)
    );
    assert_eq!(fs::read(previous).unwrap(), b"previous-attempt");
    assert!(read_metadata(root.path()).unwrap().is_none());
}
