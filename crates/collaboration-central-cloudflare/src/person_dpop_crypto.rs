//! Real isolate WebCrypto, including native ECDSA P-256, with no wasm p256 code.
use collaboration_central_core::{
    SignatureValidity, VerifyFailure,
    person_dpop_prototype::{DpopCrypto, Ed25519Jwk, Es256Jwk},
};
use js_sys::{Array, Object, futures::JsFuture};
use serde::Serialize;
use worker::wasm_bindgen::{JsCast, JsValue};

pub(crate) struct WorkerPersonDpopCrypto;

impl DpopCrypto for WorkerPersonDpopCrypto {
    async fn verify_issuer(
        &self,
        key: &Ed25519Jwk,
        signed: &[u8],
        signature: &[u8],
    ) -> Result<SignatureValidity, VerifyFailure> {
        let algorithm = object(&Algorithm {
            name: "Ed25519",
            named_curve: None,
            hash: None,
        })?;
        verify(key, &algorithm, &algorithm, signed, signature).await
    }

    async fn verify_proof(
        &self,
        key: &Es256Jwk,
        signed: &[u8],
        signature: &[u8],
    ) -> Result<SignatureValidity, VerifyFailure> {
        let import = object(&Algorithm {
            name: "ECDSA",
            named_curve: Some("P-256"),
            hash: None,
        })?;
        let verification = object(&Algorithm {
            name: "ECDSA",
            named_curve: None,
            hash: Some("SHA-256"),
        })?;
        verify(key, &import, &verification, signed, signature).await
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Algorithm {
    name: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    named_curve: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hash: Option<&'static str>,
}

fn object(value: &impl Serialize) -> Result<Object, VerifyFailure> {
    serde_wasm_bindgen::to_value(value)
        .map_err(|_| VerifyFailure::UnsupportedKey)?
        .dyn_into::<Object>()
        .map_err(|_| VerifyFailure::UnsupportedKey)
}

async fn verify(
    key: &impl Serialize,
    import: &Object,
    algorithm: &Object,
    signed: &[u8],
    signature: &[u8],
) -> Result<SignatureValidity, VerifyFailure> {
    // The host guarantees a Worker global, as in access_crypto.rs.
    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    let subtle = global.crypto().map_err(|_| VerifyFailure::Crypto)?.subtle();
    let usages = Array::new();
    usages.push(&JsValue::from_str("verify"));
    let public_key = JsFuture::from(
        subtle
            .import_key_with_object("jwk", &object(key)?, import, false, &usages)
            .map_err(|_| VerifyFailure::UnsupportedKey)?,
    )
    .await
    .map_err(|_| VerifyFailure::UnsupportedKey)?
    .dyn_into::<web_sys::CryptoKey>()
    .map_err(|_| VerifyFailure::UnsupportedKey)?;
    let result = JsFuture::from(
        subtle
            .verify_with_object_and_u8_array_and_u8_array(algorithm, &public_key, signature, signed)
            .map_err(|_| VerifyFailure::Crypto)?,
    )
    .await
    .map_err(|_| VerifyFailure::Crypto)?;
    match result.as_bool() {
        Some(true) => Ok(SignatureValidity::Valid),
        Some(false) => Ok(SignatureValidity::Invalid),
        None => Err(VerifyFailure::Crypto),
    }
}
