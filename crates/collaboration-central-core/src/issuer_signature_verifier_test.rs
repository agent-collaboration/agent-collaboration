use super::{IssuerSignatureRefusal, IssuerSignatureVerifier};
use crate::{
    Ed25519Verification, IssuerKeyRefusal, IssuerKeyResolutionRefusal, IssuerKeySource,
    KeyFetchFailure, RawJwks, SignatureValidity, VerifyFailure,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use runtime_primitives::{Clock, SyncTime};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_793_462_400_000)
    }
}

struct ControlledSource {
    answer: Result<Vec<u8>, KeyFetchFailure>,
    calls: AtomicUsize,
}

impl IssuerKeySource for ControlledSource {
    fn configured_source(&self) -> &str {
        "https://configured.example.invalid/jwks"
    }

    async fn fetch_issuer_keys(&self) -> Result<RawJwks, KeyFetchFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.answer {
            Ok(body) => Ok(RawJwks {
                body: body.clone(),
                cache_control: None,
            }),
            Err(reason) => Err(*reason),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct SignatureInvocation {
    public_key: Vec<u8>,
    signed: Vec<u8>,
    signature: Vec<u8>,
}

#[derive(Clone)]
struct RecordingCrypto {
    verdict: Result<SignatureValidity, VerifyFailure>,
    inputs: Arc<Mutex<Vec<SignatureInvocation>>>,
}

impl Ed25519Verification for RecordingCrypto {
    async fn verify_ed25519(
        &self,
        public_key: &[u8],
        signed: &[u8],
        signature: &[u8],
    ) -> Result<SignatureValidity, VerifyFailure> {
        self.inputs
            .lock()
            .map_err(|_| VerifyFailure::Crypto)?
            .push(SignatureInvocation {
                public_key: public_key.to_vec(),
                signed: signed.to_vec(),
                signature: signature.to_vec(),
            });
        self.verdict
    }
}

fn test_verifier(
    answer: Result<Vec<u8>, KeyFetchFailure>,
    verdict: Result<SignatureValidity, VerifyFailure>,
) -> IssuerSignatureVerifier<RecordingCrypto, ControlledSource> {
    IssuerSignatureVerifier::new(
        RecordingCrypto {
            verdict,
            inputs: Arc::default(),
        },
        ControlledSource {
            answer,
            calls: AtomicUsize::new(0),
        },
        Arc::default(),
        Arc::new(FixedClock),
    )
}

fn published_keys() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "keys":[{
            "kid":"issuer-key", "kty":"OKP", "crv":"Ed25519",
            "x":URL_SAFE_NO_PAD.encode([7_u8; 32])
        }]
    }))
    .unwrap()
}

// The crypto capability is controlled here. This checks composition, not Ed25519.
// A changed key, reconstructed signed bytes or altered signature must be visible.
#[tokio::test(start_paused = true)]
async fn issuer_signature_preserves_the_selected_key_and_original_signed_bytes() {
    let verifier = test_verifier(Ok(published_keys()), Ok(SignatureValidity::Valid));

    let result = verifier
        .verify("issuer-key", b"original.header-and-claims", &[2, 3, 5])
        .await;

    assert_eq!(result, Ok(SignatureValidity::Valid));
    assert_eq!(
        *verifier.platform.inputs.lock().unwrap(),
        vec![SignatureInvocation {
            public_key: vec![7; 32],
            signed: b"original.header-and-claims".to_vec(),
            signature: vec![2, 3, 5],
        }]
    );
}

#[tokio::test(start_paused = true)]
async fn issuer_signature_keeps_invalid_separate_from_unavailable_crypto() {
    for (verdict, expected) in [
        (
            Ok(SignatureValidity::Invalid),
            Ok(SignatureValidity::Invalid),
        ),
        (
            Err(VerifyFailure::Crypto),
            Err(IssuerSignatureRefusal::Crypto {
                reason: VerifyFailure::Crypto,
            }),
        ),
        (
            Err(VerifyFailure::UnsupportedKey),
            Err(IssuerSignatureRefusal::Crypto {
                reason: VerifyFailure::UnsupportedKey,
            }),
        ),
    ] {
        let verifier = test_verifier(Ok(published_keys()), verdict);

        let result = verifier.verify("issuer-key", b"signed", b"signature").await;

        assert_eq!(result, expected);
    }
}

#[tokio::test(start_paused = true)]
async fn issuer_signature_refuses_empty_or_unknown_key_without_crypto() {
    for (key_id, expected_fetches) in [("", 0), ("unknown", 1)] {
        let verifier = test_verifier(Ok(published_keys()), Ok(SignatureValidity::Valid));

        let result = verifier.verify(key_id, b"signed", b"signature").await;

        assert_eq!(result, Ok(SignatureValidity::Invalid));
        assert_eq!(
            verifier.source.calls.load(Ordering::SeqCst),
            expected_fetches
        );
        assert!(verifier.platform.inputs.lock().unwrap().is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn issuer_signature_does_not_guess_a_verdict_when_key_fetch_fails() {
    for reason in [
        KeyFetchFailure::Unreachable,
        KeyFetchFailure::BadResponse { status: 503 },
    ] {
        let verifier = test_verifier(Err(reason), Ok(SignatureValidity::Valid));

        let result = verifier.verify("issuer-key", b"signed", b"signature").await;

        assert_eq!(
            result,
            Err(IssuerSignatureRefusal::Keys(
                IssuerKeyResolutionRefusal::Fetch { reason }
            ))
        );
        assert!(verifier.platform.inputs.lock().unwrap().is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn issuer_signature_does_not_use_a_malformed_key_set() {
    let verifier = test_verifier(
        Ok(br#"{"keys":[{}]}"#.to_vec()),
        Ok(SignatureValidity::Valid),
    );

    let result = verifier.verify("issuer-key", b"signed", b"signature").await;

    assert_eq!(
        result,
        Err(IssuerSignatureRefusal::Keys(
            IssuerKeyResolutionRefusal::KeySet(IssuerKeyRefusal::MalformedKeySet)
        ))
    );
    assert!(verifier.platform.inputs.lock().unwrap().is_empty());
}
