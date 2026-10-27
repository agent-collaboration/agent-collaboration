//! Reference integrity and metadata publication use the existing encrypted test-store owner.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove retained bytes and rejected reference shapes."
)]
#![deny(clippy::too_many_lines)]
use super::*;
use codex_router_core::redaction::SecretString;
use std::{error::Error, os::unix::fs::PermissionsExt, path::Path};
type TestResult<T = ()> = Result<T, Box<dyn Error>>;

#[test]
fn cloudflare_enrollment_roundtrips_the_existing_exact_document_bytes() -> TestResult {
    let previous = r#"{"endpoint":"wss://central.example.invalid/v1/session","access":{"clientId":"fixture.access","clientSecretKey":"fleet_access_client_secret.550e8400-e29b-41d4-a716-446655440000.550e8400-e29b-41d4-a716-446655440001"},"credentialKey":"fleet_machine_credential.550e8400-e29b-41d4-a716-446655440000.550e8400-e29b-41d4-a716-446655440001","peer":"019f0000-0000-7000-8000-000000001f15","receiver":"019f0000-0000-7000-8000-000000000010","relay":"019f0000-0000-7000-8000-000000005240","expiresAt":null}"#;
    let document: FleetEnrollmentDocument = serde_json::from_str(previous)?;
    document.secret_keys()?;
    assert_eq!(serde_json::to_string(&document)?, previous);
    Ok(())
}

#[test]
fn self_hosted_reference_is_explicit_and_older_access_readers_refuse_it() -> TestResult {
    let value = serde_json::json!({"kind":"selfHosted","assertionKey":"fleet_machine_assertion.550e8400-e29b-41d4-a716-446655440000.550e8400-e29b-41d4-a716-446655440001"});
    let access: FleetAccess = serde_json::from_value(value.clone())?;
    assert!(matches!(access, FleetAccess::SelfHosted(_)));
    assert_eq!(serde_json::to_value(access)?, value);
    assert!(serde_json::from_value::<FleetAccessReference>(value).is_err());
    assert!(
        serde_json::from_value::<FleetAccess>(
            serde_json::json!({"kind":"selfHosted","clientId":"client", "clientSecretKey":"key"})
        )
        .is_err()
    );
    Ok(())
}

fn document(root: &Path) -> TestResult<FleetEnrollmentDocument> {
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
    let store = open_fleet_enrollment_store(root)?;
    store.write_secret(
        &access,
        &SecretString::new("access-secret-publication-canary"),
    )?;
    store.write_secret(
        &credential,
        &SecretString::new("Bearer machine-secret-publication-canary"),
    )?;
    Ok(FleetEnrollmentDocument {
        endpoint: "ws://127.0.0.1:1/v1/session".to_owned(),
        access: FleetAccessReference {
            client_id: "fixture.access".to_owned(),
            client_secret_key: access.as_str().to_owned(),
        }
        .into(),
        credential_key: credential.as_str().to_owned(),
        peer: "019f0000-0000-7000-8000-000000001f15".parse()?,
        receiver: "019f0000-0000-7000-8000-000000000010".parse()?,
        relay: "019f0000-0000-7000-8000-000000005240".parse()?,
        expires_at: None,
    })
}

#[test]
fn metadata_publication_is_private_and_contains_references_without_plaintext() -> TestResult {
    let root = tempfile::tempdir()?;
    let document = document(root.path())?;
    let store = open_fleet_enrollment_store(root.path())?;
    persist_fleet_enrollment(root.path(), &document, &store)?;
    let path = crate::router_fleet_enrollment_file(root.path());
    assert_eq!(path.metadata()?.permissions().mode() & 0o777, 0o600);
    let text = std::fs::read_to_string(&path)?;
    for secret in [
        "access-secret-publication-canary",
        "machine-secret-publication-canary",
    ] {
        assert!(!text.contains(secret));
    }
    assert!(text.contains("clientSecretKey"));
    assert!(text.contains("credentialKey"));
    let loaded = load_fleet_enrollment(&path, root.path())?;
    assert_eq!(
        loaded.credential.expose(),
        "Bearer machine-secret-publication-canary"
    );
    assert_eq!(
        std::fs::read_dir(root.path().join("fleet"))?.count(),
        1,
        "temporary metadata survived publication"
    );
    Ok(())
}

#[test]
fn missing_secret_preserves_previous_metadata_before_temporary_write() -> TestResult {
    let root = tempfile::tempdir()?;
    let document = document(root.path())?;
    let store = open_fleet_enrollment_store(root.path())?;
    persist_fleet_enrollment(root.path(), &document, &store)?;
    let path = crate::router_fleet_enrollment_file(root.path());
    let before = std::fs::read(&path)?;
    let (_, key) = document.secret_keys()?;
    store.delete_staged(&key)?;
    assert!(load_fleet_enrollment(&path, root.path()).is_err());
    assert!(matches!(
        persist_fleet_enrollment(root.path(), &document, &store),
        Err(FleetEnrollmentPublishError::BeforeRename(_))
    ));
    assert_eq!(std::fs::read(&path)?, before);
    assert_eq!(std::fs::read_dir(root.path().join("fleet"))?.count(), 1);
    Ok(())
}

#[test]
fn secret_reference_pair_rejects_cross_purpose_installation_and_generation() -> TestResult {
    let root = tempfile::tempdir()?;
    let original = document(root.path())?;
    let mut swapped = original.clone();
    let FleetAccess::Cloudflare(reference) = &mut swapped.access else {
        return Err("expected Cloudflare fixture".into());
    };
    std::mem::swap(
        &mut reference.client_secret_key,
        &mut swapped.credential_key,
    );
    assert!(swapped.secret_keys().is_err());
    for (installation, generation) in [
        (
            "550e8400-e29b-41d4-a716-446655440099",
            "550e8400-e29b-41d4-a716-446655440001",
        ),
        (
            "550e8400-e29b-41d4-a716-446655440000",
            "550e8400-e29b-41d4-a716-446655440099",
        ),
    ] {
        let mut changed = original.clone();
        changed.credential_key =
            FleetSecretKey::new(FleetSecretKind::MachineCredential, installation, generation)?
                .secret_key()?
                .as_str()
                .to_owned();
        assert!(changed.secret_keys().is_err());
    }
    Ok(())
}

#[test]
fn failed_rename_observes_private_nonsecret_temp_and_preserves_old_file() -> TestResult {
    let root = tempfile::tempdir()?;
    let original = document(root.path())?;
    let store = open_fleet_enrollment_store(root.path())?;
    persist_fleet_enrollment(root.path(), &original, &store)?;
    let path = crate::router_fleet_enrollment_file(root.path());
    let before = std::fs::read(&path)?;
    let mut replacement = original.clone();
    replacement.endpoint = "ws://127.0.0.1:2/v1/session".to_owned();
    let mut reached_rename = false;
    let failed = persist_fleet_enrollment_with_rename(
        root.path(),
        &replacement,
        &store,
        |temporary, destination| {
            reached_rename = true;
            assert_eq!(destination, path);
            assert_eq!(temporary.metadata()?.permissions().mode() & 0o777, 0o600);
            let bytes = std::fs::read_to_string(temporary)?;
            assert_eq!(
                serde_json::from_str::<FleetEnrollmentDocument>(&bytes)?.endpoint,
                replacement.endpoint
            );
            assert!(!bytes.contains("access-secret-publication-canary"));
            assert!(!bytes.contains("machine-secret-publication-canary"));
            Err(io::Error::other(
                "injected rename failure after complete metadata write",
            ))
        },
    );
    assert!(
        reached_rename,
        "publisher failed before the exercised boundary"
    );
    assert!(matches!(
        failed,
        Err(FleetEnrollmentPublishError::BeforeRename(_))
    ));
    assert_eq!(std::fs::read(&path)?, before);
    assert_eq!(std::fs::read_dir(root.path().join("fleet"))?.count(), 1);
    assert_eq!(
        load_fleet_enrollment(&path, root.path())?.endpoint.as_str(),
        original.endpoint
    );
    Ok(())
}

#[test]
fn directory_sync_failure_keeps_published_metadata_and_secrets_readable() -> TestResult {
    let root = tempfile::tempdir()?;
    let document = document(root.path())?;
    let store = open_fleet_enrollment_store(root.path())?;
    let parent = root.path().join("fleet");
    let published = persist_fleet_enrollment_with_rename(
        root.path(),
        &document,
        &store,
        |temporary, destination| {
            std::fs::rename(temporary, destination)?;
            // Make the following real directory open fail after the real rename.
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o000))
        },
    );
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700))?;
    assert!(matches!(
        published,
        Err(FleetEnrollmentPublishError::Renamed(_))
    ));
    let path = crate::router_fleet_enrollment_file(root.path());
    let loaded = load_fleet_enrollment(&path, root.path())?;
    assert_eq!(loaded.endpoint.as_str(), document.endpoint);
    assert_eq!(
        loaded.credential.expose(),
        "Bearer machine-secret-publication-canary"
    );
    assert_eq!(std::fs::read_dir(parent)?.count(), 1);
    Ok(())
}
