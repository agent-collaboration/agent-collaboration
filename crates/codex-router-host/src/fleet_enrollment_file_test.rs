use super::*;
use std::error::Error;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup returns errors."
)]
fn every_enrollment_durably_records_a_fresh_pending_reset_before_host_activation()
-> Result<(), Box<dyn Error>> {
    // Arrange: an actual validated enrollment and its real credential owner.
    let root = tempfile::tempdir()?;
    let source = write_fixture(root.path())?;
    let document: FleetEnrollmentDocument = serde_json::from_slice(&std::fs::read(source)?)?;
    let store = open_fleet_enrollment_store(root.path())?;
    let marker = crate::router_collaboration_directory(root.path()).join("enrollment-reset.json");

    // Act: successful publication records forgetting independently of the endpoint/ids.
    persist_fleet_enrollment(root.path(), &document, &store)?;

    // Assert: another enrollment of the same document still requires a fresh reset.
    let first: serde_json::Value = serde_json::from_slice(&std::fs::read(&marker)?)?;
    assert_eq!(first["phase"]["kind"], "pending");
    let generation = first["generation"]
        .as_str()
        .ok_or("reset generation absent")?;
    assert_eq!(uuid::Uuid::parse_str(generation)?.get_version_num(), 7);
    assert_eq!(
        std::fs::metadata(&marker)?.permissions().mode() & 0o777,
        0o600
    );
    persist_fleet_enrollment(root.path(), &document, &store)?;
    let second: serde_json::Value = serde_json::from_slice(&std::fs::read(&marker)?)?;
    assert_eq!(second["phase"]["kind"], "pending");
    assert_ne!(first["generation"], second["generation"]);
    Ok(())
}

fn write_fixture(root: &Path) -> Result<std::path::PathBuf, Box<dyn Error>> {
    use codex_router_core::redaction::SecretString;
    let store = open_fleet_enrollment_store(root)?;
    let installation = "550e8400-e29b-41d4-a716-446655440000";
    let generation = "550e8400-e29b-41d4-a716-446655440001";
    let access = FleetSecretKey::new(
        FleetSecretKind::AccessClientSecret,
        installation,
        generation,
    )?
    .secret_key()?;
    let credential =
        FleetSecretKey::new(FleetSecretKind::MachineCredential, installation, generation)?
            .secret_key()?;
    store.write_secret(&access, &SecretString::new("fixture-secret"))?;
    store.write_secret(&credential, &SecretString::new("Bearer fixture-machine"))?;
    let file = root.join("fixture-session.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&serde_json::json!({
            "endpoint":"ws://127.0.0.1:1/session",
            "access":{"clientId":"fixture-access", "clientSecretKey":access.as_str()},
            "credentialKey":credential.as_str(),
            "peer":uuid::Uuid::now_v7().to_string(),
            "receiver":uuid::Uuid::now_v7().to_string(),
            "relay":"0192a1b2-c3d4-7000-8000-000000000003",
        }))?,
    )?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
fn production_metadata_refuses_inline_access_and_machine_secrets() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let file = write_fixture(root.path())?;
    std::fs::write(
        &file,
        serde_json::to_vec(&serde_json::json!({
            "endpoint":"ws://127.0.0.1:1/session",
            "access":{"clientId":"fixture-access", "clientSecret":"fixture-secret"},
            "credential":"Bearer fixture-machine",
            "peer":uuid::Uuid::now_v7().to_string(),
            "receiver":"0192a1b2-c3d4-7000-8000-000000000002",
            "relay":"0192a1b2-c3d4-7000-8000-000000000003"
        }))?,
    )?;
    assert!(
        load_fleet_enrollment(&file, root.path()).is_err(),
        "production metadata accepted inline plaintext secrets"
    );
    Ok(())
}

#[test]
fn enrollment_accepts_optional_typed_expiry_without_owning_expiry_policy()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let file = write_fixture(root.path())?;
    let mut fixture: serde_json::Value = serde_json::from_slice(&std::fs::read(&file)?)?;
    for expiry in [
        "2000-01-01T00:00:00Z",
        "2033-05-18T03:33:20Z",
        "2033-05-18T04:33:20+01:00",
    ] {
        fixture["expiresAt"] = serde_json::json!(expiry);
        std::fs::write(&file, serde_json::to_vec(&fixture)?)?;
        load_fleet_enrollment(&file, &root.path().canonicalize()?)?;
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
fn private_fixture_preserves_typed_peer_receiver_relay_and_credentials()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let file = write_fixture(root.path())?;
    let raw: FleetEnrollmentDocument = serde_json::from_slice(&std::fs::read(&file)?)?;
    let loaded = load_fleet_enrollment(&file, &root.path().canonicalize()?)?;
    assert_eq!(loaded.peer, raw.peer);
    assert_eq!(loaded.receiver, raw.receiver);
    assert_eq!(loaded.relay, raw.relay);
    assert_eq!(loaded.endpoint.as_str(), raw.endpoint);
    assert_eq!(loaded.credential.expose(), "Bearer fixture-machine");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
fn fixture_requires_an_explicit_canonical_relay_instead_of_deriving_one()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let owned = root.path().canonicalize()?;
    let file = write_fixture(root.path())?;
    let original: serde_json::Value = serde_json::from_slice(&std::fs::read(&file)?)?;
    for relay in [
        None,
        Some(serde_json::json!("550e8400-e29b-41d4-a716-446655440000")),
    ] {
        let mut invalid = original.clone();
        let object = invalid.as_object_mut().ok_or("fixture is not an object")?;
        object.remove("relay");
        if let Some(relay) = relay {
            object.insert("relay".to_owned(), relay);
        }
        std::fs::write(&file, serde_json::to_vec(&invalid)?)?;
        assert!(load_fleet_enrollment(&file, &owned).is_err());
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
fn fixture_refuses_public_files_external_paths_and_noncanonical_identity()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let owned = root.path().canonicalize()?;
    let file = write_fixture(root.path())?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))?;
    assert!(load_fleet_enrollment(&file, &owned).is_err());
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
    let other = tempfile::tempdir()?;
    assert!(load_fleet_enrollment(&file, &other.path().canonicalize()?).is_err());
    let mut invalid: serde_json::Value = serde_json::from_slice(&std::fs::read(&file)?)?;
    let object = invalid.as_object_mut().ok_or("fixture is not an object")?;
    object.insert(
        "peer".to_owned(),
        serde_json::json!("550e8400-e29b-41d4-a716-446655440000"),
    );
    std::fs::write(&file, serde_json::to_vec(&invalid)?)?;
    assert!(load_fleet_enrollment(&file, &owned).is_err());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
fn absent_router_enrollment_is_the_only_normal_fallback() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    assert!(load_router_fleet_enrollment(root.path())?.is_none());
    std::fs::create_dir(root.path().join("fleet"))?;
    assert!(load_router_fleet_enrollment(root.path())?.is_none());
    std::os::unix::fs::symlink(
        root.path().join("absent.json"),
        root.path().join("fleet/enrollment.json"),
    )?;
    assert!(load_router_fleet_enrollment(root.path()).is_err());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
fn enrollment_refuses_a_symlink_even_when_its_target_is_private() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let file = write_fixture(root.path())?;
    let link = root.path().join("linked-enrollment.json");
    std::os::unix::fs::symlink(file, &link)?;
    assert!(load_fleet_enrollment(&link, root.path()).is_err());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
fn enrollment_refuses_relative_paths_and_directories() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    assert!(load_fleet_enrollment(Path::new("enrollment.json"), root.path()).is_err());
    assert!(load_fleet_enrollment(root.path(), root.path()).is_err());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
fn enrollment_refuses_a_parent_link_that_escapes_the_router_root() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let external = tempfile::tempdir()?;
    let file = write_fixture(external.path())?;
    let link = root.path().join("external");
    std::os::unix::fs::symlink(external.path(), &link)?;
    assert!(
        load_fleet_enrollment(&link.join(file.file_name().ok_or("filename")?), root.path())
            .is_err()
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
fn enrollment_refuses_missing_unknown_and_malformed_fields_without_echoing_secrets()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let file = write_fixture(root.path())?;
    let original: serde_json::Value = serde_json::from_slice(&std::fs::read(&file)?)?;
    for field in [
        "endpoint",
        "access",
        "credentialKey",
        "peer",
        "receiver",
        "relay",
    ] {
        let mut invalid = original.clone();
        invalid.as_object_mut().ok_or("object")?.remove(field);
        std::fs::write(&file, serde_json::to_vec(&invalid)?)?;
        assert!(
            load_fleet_enrollment(&file, root.path()).is_err(),
            "missing {field} accepted"
        );
    }
    for (field, value) in [
        ("unknown", serde_json::json!(true)),
        ("expiresAt", serde_json::json!("not-a-timestamp")),
        ("expiresAt", serde_json::json!(123)),
        (
            "peer",
            serde_json::json!("550e8400-e29b-41d4-a716-446655440000"),
        ),
        ("receiver", serde_json::json!("invalid-uuid")),
        ("relay", serde_json::json!("invalid-uuid")),
        ("endpoint", serde_json::json!("not-a-url")),
        ("credentialKey", serde_json::json!("")),
        (
            "access",
            serde_json::json!({"clientId":"", "clientSecretKey":original["access"]["clientSecretKey"]}),
        ),
        (
            "access",
            serde_json::json!({"clientId":"client", "clientSecretKey":""}),
        ),
        (
            "access",
            serde_json::json!({"clientId":"client", "clientSecretKey":"secret-sentinel\r\ninvalid"}),
        ),
        (
            "access",
            serde_json::json!({"clientId":"client", "clientSecretKey":original["access"]["clientSecretKey"], "unknown":true}),
        ),
    ] {
        let mut invalid = original.clone();
        invalid[field] = value;
        std::fs::write(&file, serde_json::to_vec(&invalid)?)?;
        let error = load_fleet_enrollment(&file, root.path())
            .err()
            .ok_or("invalid enrollment accepted")?;
        assert!(!error.to_string().contains("secret-sentinel"));
        assert!(!error.to_string().contains("fixture-machine"));
    }
    std::fs::write(&file, "{invalid-json-secret-sentinel")?;
    let error = load_fleet_enrollment(&file, root.path())
        .err()
        .ok_or("malformed JSON accepted")?;
    assert!(!error.to_string().contains("secret-sentinel"));
    Ok(())
}
