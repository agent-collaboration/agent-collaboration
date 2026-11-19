use super::{
    arguments::{CredentialKind, MintArguments},
    configuration, mint,
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use codex_router_secret_store::{
    SecretStore,
    central_credentials::{CentralCredential, CentralSecretKind},
};

fn seed_legacy_credentials(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let secret_root = root.join("secrets");
    #[cfg(debug_assertions)]
    crate::secret_store_factory::initialize_plaintext_debug_root(&secret_root)?;

    let config = configuration::CentralConfiguration {
        policy: collaboration_central_core::AccessPolicy {
            issuer: "http://127.0.0.1:29843".into(),
            jwks_url: "http://127.0.0.1:29843/.well-known/jwks.json".into(),
            session_audience: "codex-router-session".into(),
            admin_audience: "codex-router-admin".into(),
            owner_identity: "local-owner".into(),
        },
        receiver: fleet_sync_protocol::ReceiverId::new(
            "019f0000-0000-7000-8000-000000000001".parse()?,
        )?,
        relay: fleet_sync_protocol::PeerId::new("019f0000-0000-7000-8000-000000000002".parse()?)?,
    };
    let key = collaboration_central_tokio::SelfHostedIssuer::generate_signing_key()?;
    crate::secret_store_factory::open_cli_secret_store(&secret_root)?.write_staged(
        &CentralSecretKind::SigningKey.secret_key()?,
        &codex_router_core::redaction::SecretString::new(STANDARD.encode(key)),
    )?;
    std::fs::write(root.join("central.json"), serde_json::to_vec(&config)?)?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct AssertionLifetime {
    exp: i64,
    nbf: i64,
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn reported_credential_expiry_matches_the_signed_jwt_and_its_adopted_lifetime()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().to_owned();
    seed_legacy_credentials(&root)?;
    let store = crate::secret_store_factory::open_cli_secret_store(&root.join("secrets"))?;
    for (kind, secret, seconds) in [
        (
            CredentialKind::Owner,
            CentralSecretKind::OwnerCredential,
            900,
        ),
        (
            CredentialKind::Enrollment,
            CentralSecretKind::EnrollmentCredential,
            31_536_000,
        ),
    ] {
        let displayed = mint(MintArguments {
            root: root.clone(),
            kind,
            out: None,
            machine_valid_for_days: 365,
        })?;
        let encoded = store.read_secret(&secret.secret_key()?)?;
        let credential = CentralCredential::decode_secret(&encoded)?;
        let (assertion, expires_at) = match credential {
            CentralCredential::Owner {
                assertion,
                expires_at,
                ..
            }
            | CentralCredential::Enrollment {
                assertion,
                expires_at,
                ..
            } => (assertion, expires_at),
        };
        let claims = assertion.split('.').nth(1).ok_or("signed claims missing")?;
        let claims: AssertionLifetime = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims)?)?;
        let signed_expiry =
            chrono::DateTime::from_timestamp(claims.exp, 0).ok_or("invalid signed expiry")?;
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(&expires_at)?,
            signed_expiry
        );
        assert_eq!(displayed, expires_at);
        assert_eq!(claims.exp - claims.nbf, seconds);
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn minted_enrollment_credential_names_a_uuidv7_access_client()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().to_owned();
    seed_legacy_credentials(&root)?;
    let store = crate::secret_store_factory::open_cli_secret_store(&root.join("secrets"))?;

    mint(MintArguments {
        root,
        kind: CredentialKind::Enrollment,
        out: None,
        machine_valid_for_days: 365,
    })?;

    let encoded = store.read_secret(&CentralSecretKind::EnrollmentCredential.secret_key()?)?;
    let CentralCredential::Enrollment { client_id, .. } =
        CentralCredential::decode_secret(&encoded)?
    else {
        return Err("an enrollment mint stores an enrollment credential".into());
    };
    assert_eq!(uuid::Uuid::parse_str(&client_id)?.get_version_num(), 7);
    Ok(())
}
