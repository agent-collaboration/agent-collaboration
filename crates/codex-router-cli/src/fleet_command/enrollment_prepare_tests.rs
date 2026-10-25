//! Fixed vectors and rejected local inputs keep protocol hashing independent of HTTP.
#![allow(clippy::panic_in_result_fn)] // Assertions carry literal protocol expectations.
#![deny(clippy::too_many_lines)]
use super::{
    FleetCommandError,
    arguments::EnrollArguments,
    enrollment_prepare::{prepare, random_machine_token, raw_token_hash},
};
use crate::CliContext;
use chrono::{Duration, Utc};
use std::{error::Error, io::Cursor, os::unix::fs::PermissionsExt};
type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn machine_randomness_is_32_bytes_and_hashes_only_the_raw_hex_token() -> TestResult {
    let raw = random_machine_token(&mut Cursor::new([0_u8; 32]))?;
    assert_eq!(
        raw,
        "0000000000000000000000000000000000000000000000000000000000000000"
    );
    assert_eq!(
        raw_token_hash(&raw),
        "60e05bd1b195af2f94112fa7197a5c88289058840ce7c6df9693756bc6250f55"
    );
    assert_ne!(
        raw_token_hash(&raw),
        raw_token_hash(&format!("Bearer {raw}"))
    );
    assert!(matches!(
        random_machine_token(&mut Cursor::new([0_u8; 31])),
        Err(FleetCommandError::LocalIo(_))
    ));
    Ok(())
}

#[test]
fn central_admin_url_preserves_origin_and_translates_session_scheme() -> TestResult {
    let root = tempfile::tempdir()?;
    let secret = root.path().join("access");
    std::fs::write(&secret, b"access-secret-fixture\n")?;
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600))?;
    let context = CliContext::new(vec![(
        "SELECTED_OWNER_TOKEN".to_owned(),
        "owner-token-fixture".to_owned(),
    )]);
    for (endpoint, admin) in [
        (
            "wss://central.example.test:8443/v1/session",
            "https://central.example.test:8443/admin/v1/machines/enroll",
        ),
        (
            "https://central.example.test/v1/session",
            "https://central.example.test/admin/v1/machines/enroll",
        ),
        (
            "ws://127.0.0.1:8443/v1/session",
            "http://127.0.0.1:8443/admin/v1/machines/enroll",
        ),
        (
            "http://127.0.0.1:8443/v1/session",
            "http://127.0.0.1:8443/admin/v1/machines/enroll",
        ),
    ] {
        let prepared = prepare(
            EnrollArguments {
                endpoint: endpoint.to_owned(),
                label: "fixture".to_owned(),
                access_client_id: "fixture.access".to_owned(),
                access_client_secret_file: secret.clone(),
                expires_at: (Utc::now() + Duration::hours(1)).to_rfc3339(),
                owner_token_env: "SELECTED_OWNER_TOKEN".to_owned(),
                router_root: Some(root.path().join("router")),
                #[cfg(debug_assertions)]
                allow_plaintext_file_secrets: false,
            },
            &context,
        )?;
        assert_eq!(prepared.admin_url.as_str(), admin);
        assert!(prepared.owner_token.is_sensitive());
        assert_eq!(prepared.machine_credential.expose_secret().len(), 71);
    }
    Ok(())
}
