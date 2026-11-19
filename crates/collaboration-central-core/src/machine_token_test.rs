use super::*;
use crate::{IssuerKeyCache, KeyFetchFailure, RawJwks, VerifyFailure};
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use runtime_primitives::SyncTime;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

const ID: &str = "018f0000-0000-7000-8000-000000000001";
const NOW: i64 = 1_800_000_000;

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(NOW * 1000)
    }
}
#[derive(Clone)]
struct RingVerifier;
impl Ed25519Verification for RingVerifier {
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
struct KeySource {
    public: Vec<u8>,
    unavailable: bool,
    calls: Arc<AtomicUsize>,
}
impl IssuerKeySource for KeySource {
    fn configured_source(&self) -> &str {
        "https://issuer.invalid/keys"
    }
    async fn fetch_issuer_keys(&self) -> Result<RawJwks, KeyFetchFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.unavailable {
            return Err(KeyFetchFailure::Unreachable);
        }
        Ok(RawJwks { body: serde_json::to_vec(&json!({"keys":[{"kid":"key", "kty":"OKP", "crv":"Ed25519", "x": URL_SAFE_NO_PAD.encode(&self.public)}]})).unwrap(), cache_control: None })
    }
}
fn fixture(
    unavailable: bool,
) -> (
    MachineTokenVerifier<RingVerifier, KeySource>,
    Ed25519KeyPair,
) {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let clock: Arc<dyn Clock> = Arc::new(FixedClock);
    let signatures = IssuerSignatureVerifier::new(
        RingVerifier,
        KeySource {
            public: key.public_key().as_ref().to_vec(),
            unavailable,
            calls: Arc::default(),
        },
        Arc::<IssuerKeyCache>::default(),
        clock.clone(),
    );
    (
        MachineTokenVerifier::new(
            signatures,
            clock,
            MachineTokenPolicy {
                issuer: "issuer".into(),
                audience: "machine".into(),
            },
        ),
        key,
    )
}
fn claims() -> Value {
    json!({"iss":"issuer", "aud":"machine", "iat":NOW, "exp":NOW+600, "grant_expires_at":NOW+300, "organization_id":ID, "client_id":ID, "azp":ID, "principal":{"kind":"machine", "installation_id":ID, "enrollment_id":ID}, "sub":"ignored", "extension":true})
}
fn sign(key: &Ed25519KeyPair, header: Value, claims: Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
    let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
    let signed = format!("{header}.{claims}");
    format!(
        "{signed}.{}",
        URL_SAFE_NO_PAD.encode(key.sign(signed.as_bytes()).as_ref())
    )
}
fn token(key: &Ed25519KeyPair, claims: Value) -> String {
    sign(
        key,
        json!({"alg":"EdDSA", "typ":"at+jwt", "kid":"key"}),
        claims,
    )
}

#[tokio::test]
async fn signed_machine_token_returns_identity_and_the_earliest_deadline() {
    let (verifier, key) = fixture(false);
    let verified = verifier.verify(&token(&key, claims())).await.unwrap();
    assert_eq!(verified.organization_id().as_str(), ID);
    assert_eq!(verified.machine_id().as_str(), ID);
    assert_eq!(verified.enrollment_id().as_str(), ID);
    assert_eq!(verified.client_id().as_str(), ID);
    assert_eq!(verified.effective_expires_at(), UnixSeconds(NOW + 300));
}

#[tokio::test]
async fn rejects_wrong_profile_ids_and_grant_without_leeway() {
    let (verifier, key) = fixture(false);
    let mut invalid = Vec::new();
    for (field, value) in [
        ("aud", json!(["machine"])),
        ("iss", json!("other")),
        ("client_id", json!("bad")),
        ("azp", json!("bad")),
        ("grant_expires_at", json!(NOW)),
        ("iat", json!(NOW + 61)),
        ("exp", json!(NOW - 60)),
        ("exp", json!(NOW + 601)),
        ("workspace_id", Value::Null),
        ("sid", json!("session")),
        ("iat", json!(-1)),
        ("exp", json!(MAX_SAFE_INTEGER)),
    ] {
        let mut claim = claims();
        claim[field] = value;
        invalid.push(claim);
    }
    let mut principal = claims();
    principal["principal"]["extra"] = json!(true);
    invalid.push(principal);
    let mut machine_id = claims();
    machine_id["principal"]["installation_id"] = json!("legacy");
    invalid.push(machine_id);
    let mut wrong_client = claims();
    let other_client = json!("019a61bb-0000-7000-8000-000000000099");
    wrong_client["client_id"] = other_client.clone();
    wrong_client["azp"] = other_client;
    invalid.push(wrong_client);
    for claim in invalid {
        assert_eq!(
            verifier.verify(&token(&key, claim)).await,
            Err(MachineTokenRefusal::Rejected)
        );
    }
    let mut null_sid = claims();
    null_sid["sid"] = Value::Null;
    assert!(verifier.verify(&token(&key, null_sid)).await.is_ok());
}

#[tokio::test]
async fn rejects_tampered_signatures_and_distinguishes_unavailable_keys() {
    let (verifier, key) = fixture(false);
    let other_key = Ed25519KeyPair::from_seed_unchecked(&[8; 32]).unwrap();
    assert_eq!(
        verifier.verify(&token(&other_key, claims())).await,
        Err(MachineTokenRefusal::Rejected)
    );
    assert_eq!(
        verifier
            .verify(&sign(
                &key,
                json!({"alg":"EdDSA", "typ":"JWT", "kid":"key"}),
                claims()
            ))
            .await,
        Err(MachineTokenRefusal::Rejected)
    );
    let (unavailable, _) = fixture(true);
    assert_eq!(
        unavailable.verify(&token(&key, claims())).await,
        Err(MachineTokenRefusal::Unavailable)
    );
}

#[tokio::test]
async fn token_expiry_has_sixty_seconds_leeway_but_grant_expiry_does_not() {
    let (verifier, key) = fixture(false);
    let mut claim = claims();
    claim["iat"] = json!(NOW - 600);
    claim["exp"] = json!(NOW - 59);
    claim["grant_expires_at"] = json!(NOW + 30);
    let checked = verifier.verify(&token(&key, claim.clone())).await.unwrap();
    assert_eq!(checked.effective_expires_at(), UnixSeconds(NOW + 1));
    claim["exp"] = json!(NOW - 60);
    assert_eq!(
        verifier.verify(&token(&key, claim.clone())).await,
        Err(MachineTokenRefusal::Rejected)
    );
    claim["exp"] = json!(NOW + 60);
    claim["grant_expires_at"] = json!(NOW);
    assert_eq!(
        verifier.verify(&token(&key, claim)).await,
        Err(MachineTokenRefusal::Rejected)
    );
}

#[tokio::test]
async fn accepts_exact_issued_at_and_lifetime_limits_but_rejects_bad_headers() {
    let (verifier, key) = fixture(false);
    let mut claim = claims();
    claim["iat"] = json!(NOW + 60);
    claim["exp"] = json!(NOW + 660);
    assert!(verifier.verify(&token(&key, claim)).await.is_ok());
    for header in [
        json!({"alg":"none", "typ":"at+jwt", "kid":"key"}),
        json!({"alg":"EdDSA", "kid":"key"}),
        json!({"alg":"EdDSA", "typ":"at+jwt", "kid":""}),
        json!({"alg":"EdDSA", "typ":"at+jwt", "kid":"unknown"}),
    ] {
        assert_eq!(
            verifier.verify(&sign(&key, header, claims())).await,
            Err(MachineTokenRefusal::Rejected)
        );
    }
}

#[tokio::test]
async fn wrong_issuer_is_rejected_before_fetch_and_empty_policy_is_unavailable() {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let clock: Arc<dyn Clock> = Arc::new(FixedClock);
    let calls = Arc::new(AtomicUsize::new(0));
    let signatures = IssuerSignatureVerifier::new(
        RingVerifier,
        KeySource {
            public: key.public_key().as_ref().to_vec(),
            unavailable: false,
            calls: calls.clone(),
        },
        Arc::<IssuerKeyCache>::default(),
        clock.clone(),
    );
    let mut verifier = MachineTokenVerifier::new(
        signatures,
        clock,
        MachineTokenPolicy {
            issuer: "issuer".into(),
            audience: "machine".into(),
        },
    );
    let mut wrong_issuer = claims();
    wrong_issuer["iss"] = json!("other");
    assert_eq!(
        verifier.verify(&token(&key, wrong_issuer)).await,
        Err(MachineTokenRefusal::Rejected)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    verifier.policy.audience.clear();
    assert_eq!(
        verifier.verify(&token(&key, claims())).await,
        Err(MachineTokenRefusal::Unavailable)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn duplicate_private_claim_is_rejected_even_when_both_values_match() {
    let (verifier, key) = fixture(false);
    let encoded_header = URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","typ":"at+jwt","kid":"key"}"#);
    let claim_body = serde_json::to_string(&claims()).unwrap();
    let duplicate = format!(
        "{{\"organization_id\":\"{ID}\",{}",
        claim_body.strip_prefix('{').unwrap()
    );
    let encoded_claims = URL_SAFE_NO_PAD.encode(duplicate);
    let signed = format!("{encoded_header}.{encoded_claims}");
    let token = format!(
        "{signed}.{}",
        URL_SAFE_NO_PAD.encode(key.sign(signed.as_bytes()).as_ref())
    );
    assert_eq!(
        verifier.verify(&token).await,
        Err(MachineTokenRefusal::Rejected)
    );
}

#[test]
fn effective_deadline_must_fit_in_safe_milliseconds() {
    let policy = MachineTokenPolicy {
        issuer: "issuer".into(),
        audience: "machine".into(),
    };
    let limit = MAX_SAFE_INTEGER / 1000;
    let mut claim = claims();
    claim["iat"] = json!(limit - MACHINE_TOKEN_LIFETIME_SECONDS);
    claim["exp"] = json!(limit);
    claim["grant_expires_at"] = json!(limit);
    assert_eq!(
        serde_json::from_value::<Claims>(claim.clone())
            .unwrap()
            .validate(&policy, limit - MACHINE_TOKEN_LIFETIME_SECONDS)
            .unwrap()
            .effective_expires_at(),
        UnixSeconds(limit)
    );
    claim["grant_expires_at"] = json!(limit + 1);
    assert_eq!(
        serde_json::from_value::<Claims>(claim)
            .unwrap()
            .validate(&policy, limit - MACHINE_TOKEN_LIFETIME_SECONDS),
        Err(MachineTokenRefusal::Rejected)
    );
}
