//! Ed25519 verification for issuer tokens; key selection stays in the core.
use collaboration_central_core::SignatureValidity;
use ring::signature::{ED25519, UnparsedPublicKey};

pub(crate) fn verify_ed25519_signature(
    public_key: &[u8],
    signed: &[u8],
    signature: &[u8],
) -> SignatureValidity {
    if UnparsedPublicKey::new(&ED25519, public_key)
        .verify(signed, signature)
        .is_ok()
    {
        SignatureValidity::Valid
    } else {
        SignatureValidity::Invalid
    }
}
