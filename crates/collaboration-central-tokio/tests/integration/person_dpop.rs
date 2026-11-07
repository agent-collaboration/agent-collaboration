//! JOSE producer bytes checked by the actual Tokio crypto adapter and pure core.
use collaboration_central_core::person_dpop_prototype::{
    DpopCrypto, DpopRefusal, DpopVerification, Ed25519Jwk, Es256Jwk, PersonDpopInput,
    PersonDpopPolicy, verify_person_request,
};
use collaboration_central_core::{SignatureValidity, VerifyFailure};
use collaboration_central_tokio::NativePersonDpopCrypto;
use runtime_primitives::{Clock, SyncTime};
use serde::Deserialize;
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

#[derive(Deserialize)]
struct Fixtures {
    policy: PersonDpopPolicy,
    trusted_jwks: String,
    thumbprint: String,
    intent: String,
    vectors: Vec<Vector>,
}
#[derive(Deserialize)]
struct Vector {
    name: String,
    input: PersonDpopInput,
    now_ms: i64,
    expected: String,
}
struct FixedClock(i64);
impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0)
    }
}

#[tokio::test]
async fn jose_vectors_cross_native_crypto_and_core() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange: fixed TS JOSE signatures, RFC 7638 thumbprint and canonical intent.
    let fixtures: Fixtures = serde_json::from_str(include_str!(
        "../../../collaboration-central-core/tests/fixtures/person-dpop.json"
    ))?;
    assert!(fixtures.vectors.len() >= 40, "missing prototype scenarios");
    for vector in &fixtures.vectors {
        // Act: no mocked crypto and no host wall clock.
        let result = verify_person_request(
            &NativePersonDpopCrypto,
            &FixedClock(vector.now_ms),
            &fixtures.policy,
            &fixtures.trusted_jwks,
            &vector.input,
        )
        .await;
        // Assert: a rejection for the intended rule, not just any refusal.
        match result {
            DpopVerification::Verified { request } => {
                assert_eq!(vector.expected, "verified", "{}", vector.name);
                assert_eq!(
                    request.proof_key_thumbprint, fixtures.thumbprint,
                    "{}",
                    vector.name
                );
                assert_eq!(request.intent_digest, fixtures.intent, "{}", vector.name);
                assert_eq!(
                    request.request_id, vector.input.request_id,
                    "{}",
                    vector.name
                );
            }
            DpopVerification::Refused { reason } => {
                assert_eq!(
                    serde_json::to_value(reason)?,
                    vector.expected,
                    "{}",
                    vector.name
                );
            }
        }
    }
    Ok(())
}

struct AdvancingCrypto {
    clock: Arc<AtomicI64>,
    expiry_ms: i64,
}
impl DpopCrypto for AdvancingCrypto {
    async fn verify_issuer(
        &self,
        key: &Ed25519Jwk,
        signed: &[u8],
        signature: &[u8],
    ) -> Result<SignatureValidity, VerifyFailure> {
        NativePersonDpopCrypto
            .verify_issuer(key, signed, signature)
            .await
    }
    async fn verify_proof(
        &self,
        key: &Es256Jwk,
        signed: &[u8],
        signature: &[u8],
    ) -> Result<SignatureValidity, VerifyFailure> {
        let result = NativePersonDpopCrypto
            .verify_proof(key, signed, signature)
            .await;
        self.clock.store(self.expiry_ms, Ordering::SeqCst);
        result
    }
}
struct ApplyingClock(Arc<AtomicI64>);
impl Clock for ApplyingClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}

#[tokio::test]
async fn expiry_is_sampled_after_host_crypto_finishes() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange: valid at ingress, but host crypto finishes at the signed deadline.
    let fixtures: Fixtures = serde_json::from_str(include_str!(
        "../../../collaboration-central-core/tests/fixtures/person-dpop.json"
    ))?;
    let vector = fixtures
        .vectors
        .iter()
        .find(|vector| vector.name == "valid")
        .ok_or("missing valid vector")?;
    let clock = Arc::new(AtomicI64::new(vector.now_ms));
    let crypto = AdvancingCrypto {
        clock: Arc::clone(&clock),
        expiry_ms: vector.now_ms + 290_000,
    };
    // Act: real native signatures, with a controlled passage of validation time.
    let result = verify_person_request(
        &crypto,
        &ApplyingClock(clock),
        &fixtures.policy,
        &fixtures.trusted_jwks,
        &vector.input,
    )
    .await;
    // Assert: an earlier ingress clock cannot extend the snapshot deadline.
    assert_eq!(
        result,
        DpopVerification::Refused {
            reason: DpopRefusal::Expired
        }
    );
    Ok(())
}
