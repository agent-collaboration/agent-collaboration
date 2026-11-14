use super::{NativeEd25519, NativeEs256};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{
    Ed25519Verification, SignatureValidity, VerifyFailure,
    person_request::{Es256Verification, ProofJwk},
};
use ring::{
    rand::SystemRandom,
    signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, Ed25519KeyPair, KeyPair},
};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions check real signature outcomes while fixture errors propagate."
)]
#[tokio::test]
async fn ed25519_rejects_a_changed_message_and_an_unpublished_signer() -> TestResult {
    // Arrange: fixed public test fixture seeds, never deployment keys.
    let signer =
        Ed25519KeyPair::from_seed_unchecked(&[7; 32]).map_err(|_| "fixture Ed25519 key")?;
    let other_signer =
        Ed25519KeyPair::from_seed_unchecked(&[8; 32]).map_err(|_| "fixture second Ed25519 key")?;
    let message = b"signed organization statement";
    let signature = signer.sign(message);

    // Act / Assert: these outcomes fail if the adapter ignores signatures.
    assert_eq!(
        NativeEd25519
            .verify_ed25519(signer.public_key().as_ref(), message, signature.as_ref())
            .await,
        Ok(SignatureValidity::Valid)
    );
    assert_eq!(
        NativeEd25519
            .verify_ed25519(signer.public_key().as_ref(), b"changed", signature.as_ref())
            .await,
        Ok(SignatureValidity::Invalid)
    );
    assert_eq!(
        NativeEd25519
            .verify_ed25519(
                other_signer.public_key().as_ref(),
                message,
                signature.as_ref()
            )
            .await,
        Ok(SignatureValidity::Invalid)
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions check fixed-width ES256 proof outcomes while fixture errors propagate."
)]
#[tokio::test]
async fn es256_verifies_fixed_width_proofs_and_refuses_tampering() -> TestResult {
    // Arrange: the signer produces the JWS fixed-width signature the host accepts.
    let random = SystemRandom::new();
    let document = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &random)
        .map_err(|_| "fixture P-256 document")?;
    let signer =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, document.as_ref(), &random)
            .map_err(|_| "fixture P-256 signer")?;
    let public_key = signer.public_key().as_ref();
    let key = ProofJwk {
        kty: "EC".into(),
        crv: "P-256".into(),
        x: URL_SAFE_NO_PAD.encode(public_key.get(1..33).ok_or("fixture X")?),
        y: URL_SAFE_NO_PAD.encode(public_key.get(33..65).ok_or("fixture Y")?),
    };
    let message = b"signed DPoP proof";
    let signature = signer.sign(&random, message).map_err(|_| "fixture proof")?;

    // Act / Assert: both acceptance and rejection go through ring verification.
    assert_eq!(
        NativeEs256
            .verify_es256(&key, message, signature.as_ref())
            .await,
        Ok(SignatureValidity::Valid)
    );
    assert_eq!(
        NativeEs256
            .verify_es256(&key, b"changed", signature.as_ref())
            .await,
        Ok(SignatureValidity::Invalid)
    );
    let malformed = ProofJwk {
        x: "!".into(),
        ..key
    };
    assert_eq!(
        NativeEs256
            .verify_es256(&malformed, message, signature.as_ref())
            .await,
        Err(VerifyFailure::UnsupportedKey)
    );
    Ok(())
}
