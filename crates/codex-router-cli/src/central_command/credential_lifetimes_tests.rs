#![expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions fail the test while fixture errors propagate."
)]
use super::{
    arguments::{CredentialKind, MintArguments, ServeArguments},
    configuration, mint,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use codex_router_secret_store::{
    SecretStore,
    central_credentials::{CentralCredential, CentralSecretKind},
};

#[derive(serde::Deserialize)]
struct AssertionLifetime {
    exp: i64,
    nbf: i64,
}

#[test]
fn reported_credential_expiry_matches_the_signed_jwt_and_its_adopted_lifetime()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().to_owned();
    configuration::initialize(&ServeArguments {
        root: root.clone(),
        listen: "127.0.0.1:29843".parse()?,
        public_url: None,
        owner_identity: None,
        #[cfg(debug_assertions)]
        allow_plaintext_file_secrets: true,
    })?;
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
