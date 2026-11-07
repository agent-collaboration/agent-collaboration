//! JOSE producer bytes checked by the actual Tokio crypto adapter and pure core.
use collaboration_central_core::person_dpop_prototype::{
    DpopVerification, PersonDpopInput, PersonDpopPolicy, verify_person_request,
};
use collaboration_central_tokio::NativePersonDpopCrypto;
use runtime_primitives::{Clock, SyncTime};
use serde::Deserialize;

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
