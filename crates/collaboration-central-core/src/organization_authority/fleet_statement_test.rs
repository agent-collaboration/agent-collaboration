use super::{
    FleetOrganizationStatementRefusal as Refusal, FleetOrganizationStatementTrust,
    FleetOrganizationStatementVerifier, VerifiedFleetOrganizationStatement,
};
use crate::{
    Ed25519Verification, IssuerKeyCache, IssuerKeySource, KeyFetchFailure, RawJwks,
    SignatureValidity, VerifyFailure,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use fleet_identity_protocol::OrganizationId;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use runtime_primitives::{Clock, SyncTime};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

pub(super) const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000ab01";
pub(super) const FOREIGN_ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000ab02";
const NOW: i64 = 1_800_000_000_000;

struct ControlledClock(AtomicI64);
impl Clock for ControlledClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}

#[derive(Clone)]
struct RealEd25519;
impl Ed25519Verification for RealEd25519 {
    async fn verify_ed25519(
        &self,
        key: &[u8],
        signed: &[u8],
        signature: &[u8],
    ) -> Result<SignatureValidity, VerifyFailure> {
        Ok(
            if UnparsedPublicKey::new(&ED25519, key)
                .verify(signed, signature)
                .is_ok()
            {
                SignatureValidity::Valid
            } else {
                SignatureValidity::Invalid
            },
        )
    }
}

struct FixtureKeys;
impl IssuerKeySource for FixtureKeys {
    fn configured_source(&self) -> &str {
        "https://sign-in.example.invalid/api/auth/jwks"
    }
    async fn fetch_issuer_keys(&self) -> Result<RawJwks, KeyFetchFailure> {
        let key = signing_key();
        Ok(RawJwks {
            body: serde_json::to_vec(&json!({"keys":[{
                "kid":"stamp-fixture", "kty":"OKP", "crv":"Ed25519", "alg":"EdDSA",
                "x":URL_SAFE_NO_PAD.encode(key.public_key().as_ref())
            }]}))
            .map_err(|_| KeyFetchFailure::Unreachable)?,
            cache_control: None,
        })
    }
}

fn signing_key() -> Ed25519KeyPair {
    // Public test fixture only: never a deployment/private-user signing key.
    Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap()
}

pub(super) fn claims() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/fleet_organization_statement.json"
    ))
    .unwrap()
}
fn header() -> Value {
    json!({"typ":"fleet-organization-stamp+jwt", "alg":"EdDSA", "kid":"stamp-fixture"})
}

pub(super) fn signed_statement(header: &Value, claims: &Value) -> String {
    let signed = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(header).unwrap()),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap())
    );
    format!(
        "{signed}.{}",
        URL_SAFE_NO_PAD.encode(signing_key().sign(signed.as_bytes()).as_ref())
    )
}

fn verifier() -> (
    FleetOrganizationStatementVerifier<RealEd25519, FixtureKeys>,
    Arc<ControlledClock>,
) {
    let clock = Arc::new(ControlledClock(AtomicI64::new(NOW)));
    let trust = FleetOrganizationStatementTrust::new(
        "https://sign-in.example.invalid".into(),
        "https://central.example.invalid/identity".into(),
        organization(ORGANIZATION),
    )
    .unwrap();
    (
        FleetOrganizationStatementVerifier::new(
            RealEd25519,
            FixtureKeys,
            Arc::new(IssuerKeyCache::default()),
            clock.clone(),
            trust,
        ),
        clock,
    )
}

pub(super) fn organization(value: &str) -> OrganizationId {
    value.to_owned().try_into().unwrap()
}

pub(super) async fn verified_fixture() -> VerifiedFleetOrganizationStatement {
    verifier()
        .0
        .verify(&signed_statement(&header(), &claims()))
        .await
        .unwrap()
}

pub(super) async fn verified_claims(input: Value) -> VerifiedFleetOrganizationStatement {
    verifier()
        .0
        .verify(&signed_statement(&header(), &input))
        .await
        .unwrap()
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_accepts_a_real_signature_with_the_exact_sign_in_profile() {
    let (verifier, _) = verifier();
    let statement = signed_statement(&header(), &claims());
    let verified = verifier.verify(&statement).await.unwrap();
    assert_eq!(verified.organization_id(), &organization(ORGANIZATION));
    assert_eq!(verified.key_id(), "stamp-fixture");
    assert_eq!(verified.expires_at().0, 1_800_000_600);
    assert!(verified.replaces().is_none());
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_refuses_a_forged_signature() {
    let (verifier, _) = verifier();
    let valid = signed_statement(&header(), &claims());
    let (signed, _) = valid.rsplit_once('.').unwrap();
    let forged = format!("{signed}.{}", URL_SAFE_NO_PAD.encode([0; 64]));
    assert_eq!(
        verifier.verify(&forged).await.unwrap_err(),
        Refusal::BadSignature
    );
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_refuses_a_person_token_header() {
    let (verifier, _) = verifier();
    let mut input = header();
    input["typ"] = json!("at+jwt");
    let statement = signed_statement(&input, &claims());
    assert_eq!(
        verifier.verify(&statement).await.unwrap_err(),
        Refusal::WrongTokenType
    );
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_refuses_another_signed_purpose() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["purpose"] = json!("person-token");
    let statement = signed_statement(&header(), &input);
    assert_eq!(
        verifier.verify(&statement).await.unwrap_err(),
        Refusal::WrongPurpose
    );
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_refuses_another_audience() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["aud"] = json!("https://another.example.invalid/identity");
    assert_eq!(
        verifier
            .verify(&signed_statement(&header(), &input))
            .await
            .unwrap_err(),
        Refusal::WrongAudience
    );
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_expires_at_the_exact_exp_boundary() {
    let (verifier, clock) = verifier();
    let statement = signed_statement(&header(), &claims());
    clock.0.store(1_800_000_599_999, Ordering::SeqCst);
    assert!(verifier.verify(&statement).await.is_ok());
    clock.0.store(1_800_000_600_000, Ordering::SeqCst);
    assert_eq!(
        verifier.verify(&statement).await.unwrap_err(),
        Refusal::Expired
    );
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_refuses_another_organization_even_with_its_real_signature() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["organization_id"] = json!(FOREIGN_ORGANIZATION);
    input["sub"] = json!(FOREIGN_ORGANIZATION);
    assert_eq!(
        verifier
            .verify(&signed_statement(&header(), &input))
            .await
            .unwrap_err(),
        Refusal::OrganizationMismatch
    );
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_requires_subject_and_organization_to_agree() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["sub"] = json!(FOREIGN_ORGANIZATION);
    assert_eq!(
        verifier
            .verify(&signed_statement(&header(), &input))
            .await
            .unwrap_err(),
        Refusal::OrganizationMismatch
    );
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_refuses_a_different_issuer() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["iss"] = json!("https://another.example.invalid");
    assert_eq!(
        verifier
            .verify(&signed_statement(&header(), &input))
            .await
            .unwrap_err(),
        Refusal::WrongIssuer
    );
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_refuses_an_unsupported_version() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["version"] = json!(2);
    assert_eq!(
        verifier
            .verify(&signed_statement(&header(), &input))
            .await
            .unwrap_err(),
        Refusal::UnsupportedVersion
    );
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_refuses_a_future_issuance_time() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["iat"] = json!(1_800_000_001);
    assert_eq!(
        verifier
            .verify(&signed_statement(&header(), &input))
            .await
            .unwrap_err(),
        Refusal::NotYetValid
    );
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_refuses_another_signature_algorithm() {
    let (verifier, _) = verifier();
    let mut input = header();
    input["alg"] = json!("RS256");
    assert_eq!(
        verifier
            .verify(&signed_statement(&input, &claims()))
            .await
            .unwrap_err(),
        Refusal::UnsupportedAlgorithm
    );
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_keeps_a_valid_signed_predecessor_without_applying_policy() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["replaces"] = json!("ab".repeat(32));
    let verified = verifier
        .verify(&signed_statement(&header(), &input))
        .await
        .unwrap();
    assert_eq!(verified.replaces().unwrap().as_str(), "ab".repeat(32));
}

#[tokio::test(start_paused = true)]
async fn fleet_statement_refuses_null_malformed_or_unsigned_profile_fields() {
    let (verifier, _) = verifier();
    for (field, value) in [
        ("replaces", Value::Null),
        ("replaces", json!("AB".repeat(32))),
        ("sid", json!("person-session")),
        ("aud", json!(["https://central.example.invalid/identity"])),
        ("exp", json!(9_007_199_254_740_992_i64)),
        ("iat", json!(-1)),
    ] {
        let mut input = claims();
        input[field] = value;
        assert_eq!(
            verifier
                .verify(&signed_statement(&header(), &input))
                .await
                .unwrap_err(),
            Refusal::Malformed
        );
    }
    let mut input = header();
    input["crit"] = json!(["future"]);
    assert_eq!(
        verifier
            .verify(&signed_statement(&input, &claims()))
            .await
            .unwrap_err(),
        Refusal::Malformed
    );
}
