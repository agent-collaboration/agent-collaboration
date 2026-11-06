use super::{
    FleetOrganizationStatementTrust, FleetOrganizationStatementVerifier,
    VerifiedFleetOrganizationStatement,
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

pub(crate) const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000ab01";
pub(crate) const FOREIGN_ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000ab02";
const NOW: i64 = 1_800_000_000_000;

pub(crate) struct ControlledClock(pub(crate) AtomicI64);
impl Clock for ControlledClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}

#[derive(Clone)]
pub(crate) struct RealEd25519;
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

pub(crate) struct FixtureKeys;
impl IssuerKeySource for FixtureKeys {
    fn configured_source(&self) -> &str {
        "https://sign-in.example.invalid/api/auth/jwks"
    }
    async fn fetch_issuer_keys(&self) -> Result<RawJwks, KeyFetchFailure> {
        let key = signing_key();
        let next_key = Ed25519KeyPair::from_seed_unchecked(&[8; 32])
            .map_err(|_| KeyFetchFailure::Unreachable)?;
        Ok(RawJwks {
            body: serde_json::to_vec(&json!({"keys":[{
                "kid":"stamp-fixture", "kty":"OKP", "crv":"Ed25519", "alg":"EdDSA",
                "x":URL_SAFE_NO_PAD.encode(key.public_key().as_ref())
            }, {
                "kid":"next-stamp-fixture", "kty":"OKP", "crv":"Ed25519", "alg":"EdDSA",
                "x":URL_SAFE_NO_PAD.encode(next_key.public_key().as_ref())
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

pub(crate) fn claims() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/fleet_organization_statement.json"
    ))
    .unwrap()
}
pub(crate) fn header() -> Value {
    json!({"typ":"fleet-organization-stamp+jwt", "alg":"EdDSA", "kid":"stamp-fixture"})
}

pub(crate) fn signed_statement(header: &Value, claims: &Value) -> String {
    let key = if header.get("kid").and_then(Value::as_str) == Some("next-stamp-fixture") {
        Ed25519KeyPair::from_seed_unchecked(&[8; 32]).unwrap()
    } else {
        signing_key()
    };
    let signed = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(header).unwrap()),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap())
    );
    format!(
        "{signed}.{}",
        URL_SAFE_NO_PAD.encode(key.sign(signed.as_bytes()).as_ref())
    )
}

pub(crate) fn verifier() -> (
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

pub(crate) fn organization(value: &str) -> OrganizationId {
    value.to_owned().try_into().unwrap()
}

pub(crate) async fn verified_fixture() -> VerifiedFleetOrganizationStatement {
    verifier()
        .0
        .verify(&signed_statement(&header(), &claims()))
        .await
        .unwrap()
}

pub(crate) async fn verified_claims(input: Value) -> VerifiedFleetOrganizationStatement {
    verifier()
        .0
        .verify(&signed_statement(&header(), &input))
        .await
        .unwrap()
}

pub(crate) async fn verified_next_key(input: Value) -> VerifiedFleetOrganizationStatement {
    let mut next_header = header();
    next_header["kid"] = json!("next-stamp-fixture");
    verifier()
        .0
        .verify(&signed_statement(&next_header, &input))
        .await
        .unwrap()
}

pub(crate) async fn verified_foreign_organization() -> VerifiedFleetOrganizationStatement {
    let mut input = claims();
    input["sub"] = json!(FOREIGN_ORGANIZATION);
    input["organization_id"] = json!(FOREIGN_ORGANIZATION);
    let trust = FleetOrganizationStatementTrust::new(
        "https://sign-in.example.invalid".into(),
        "https://central.example.invalid/identity".into(),
        organization(FOREIGN_ORGANIZATION),
    )
    .unwrap();
    FleetOrganizationStatementVerifier::new(
        RealEd25519,
        FixtureKeys,
        Arc::default(),
        Arc::new(ControlledClock(AtomicI64::new(NOW))),
        trust,
    )
    .verify(&signed_statement(&header(), &input))
    .await
    .unwrap()
}
