use super::*;
use codex_router_core::redaction::SecretString;
use collaboration_central_core::{AccessClientId, EnrollMachine};
use fleet_sync_protocol::{PeerId, ReceiverId};
use reqwest::{Url, header::HeaderValue};
use std::os::unix::fs::DirBuilderExt as _;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn published_enrollment_names_its_secrets_by_a_uuidv7_generation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let root = directory.path().to_owned();
    let collaboration = codex_router_host::router_collaboration_directory(&root);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&collaboration)?;
    let installation = String::from(collaboration_runtime::load_service_identity(
        &collaboration,
    )?)
    .try_into()
    .map_err(|_| "installation identity")?;
    let peer = collaboration_runtime::load_installation_peer(&collaboration)?;
    let expires_at = chrono::Utc::now() + chrono::Duration::days(1);
    let prepared = PreparedEnrollment {
        root: root.clone(),
        endpoint: "wss://central.example.invalid/session".to_owned(),
        admin_url: Url::parse("https://central.example.invalid/admin/v1/machines/enroll")?,
        request: EnrollMachine {
            installation,
            peer,
            label: "Fixture machine"
                .to_owned()
                .try_into()
                .map_err(|_| "machine label")?,
            access_client_id: AccessClientId::new("fixture-access-client")
                .map_err(|_| "access client id")?,
            credential_hash: "0".repeat(64),
            expires_at,
        },
        owner_token: HeaderValue::from_static("fixture-owner-token"),
        owner_header: "authorization",
        access: PreparedAccess::SelfHosted(SecretString::new("fixture-assertion".to_owned())),
        machine_credential: SecretString::new(format!("Bearer {}", "a".repeat(64))),
        #[cfg(debug_assertions)]
        allow_plaintext_file_secrets: true,
    };

    publish(
        prepared,
        &EnrollmentDecision::MachineEnrolled {
            peer,
            expires_at,
            authoritative_receiver: ReceiverId::new(uuid::Uuid::now_v7())?,
            relay_peer: PeerId::new(uuid::Uuid::now_v7())?,
        },
    )?;

    let document: serde_json::Value = serde_json::from_slice(&std::fs::read(
        codex_router_host::router_fleet_enrollment_file(&root),
    )?)?;
    let credential_key = document["credentialKey"]
        .as_str()
        .ok_or("published credential key")?;
    let generation = credential_key
        .rsplit('.')
        .next()
        .ok_or("credential key generation")?;
    assert_eq!(uuid::Uuid::parse_str(generation)?.get_version_num(), 7);
    Ok(())
}
