//! WebCrypto signature mechanics; core owns sign-in key and token profiles.

use collaboration_central_core::{
    Ed25519Verification, SignatureValidity, VerifyFailure,
    person_request::{Es256Verification, ProofJwk},
};
use js_sys::{Array, Object, futures::JsFuture};
use serde::Serialize;
use std::future::Future;
use worker::{
    send::SendFuture,
    wasm_bindgen::{JsCast, JsValue},
};

#[derive(Clone)]
pub(crate) struct WorkerEd25519;

impl Ed25519Verification for WorkerEd25519 {
    fn verify_ed25519(
        &self,
        public_key: &[u8],
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send {
        let public_key = public_key.to_vec();
        let signed = signed.to_vec();
        let signature = signature.to_vec();
        SendFuture::new(
            async move { verify_ed25519_signature(&public_key, &signed, &signature).await },
        )
    }
}

#[derive(Serialize)]
struct EdAlgorithm {
    name: &'static str,
}

async fn verify_ed25519_signature(
    public_key: &[u8],
    signed: &[u8],
    signature: &[u8],
) -> Result<SignatureValidity, VerifyFailure> {
    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    let subtle = global.crypto().map_err(|_| VerifyFailure::Crypto)?.subtle();
    let algorithm = serde_wasm_bindgen::to_value(&EdAlgorithm { name: "Ed25519" })
        .map_err(|_| VerifyFailure::UnsupportedKey)?
        .dyn_into::<Object>()
        .map_err(|_| VerifyFailure::UnsupportedKey)?;
    let usages = Array::new();
    usages.push(&JsValue::from_str("verify"));
    let raw = js_sys::Uint8Array::from(public_key);
    let key = JsFuture::from(
        subtle
            .import_key_with_object("raw", &raw, &algorithm, false, &usages)
            .map_err(|_| VerifyFailure::UnsupportedKey)?,
    )
    .await
    .map_err(|_| VerifyFailure::UnsupportedKey)?
    .dyn_into::<web_sys::CryptoKey>()
    .map_err(|_| VerifyFailure::UnsupportedKey)?;
    let result = JsFuture::from(
        subtle
            .verify_with_object_and_u8_array_and_u8_array(&algorithm, &key, signature, signed)
            .map_err(|_| VerifyFailure::Crypto)?,
    )
    .await
    .map_err(|_| VerifyFailure::Crypto)?;
    signature_validity(result)
}

#[derive(Clone)]
pub(crate) struct WorkerEs256;

impl Es256Verification for WorkerEs256 {
    fn verify_es256(
        &self,
        key: &ProofJwk,
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send {
        let key = key.clone();
        let signed = signed.to_vec();
        let signature = signature.to_vec();
        SendFuture::new(async move { verify_es256_signature(&key, &signed, &signature).await })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EcAlgorithm {
    name: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    named_curve: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hash: Option<&'static str>,
}

async fn verify_es256_signature(
    key: &ProofJwk,
    signed: &[u8],
    signature: &[u8],
) -> Result<SignatureValidity, VerifyFailure> {
    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    let subtle = global.crypto().map_err(|_| VerifyFailure::Crypto)?.subtle();
    let to_object = |value: JsValue| {
        value
            .dyn_into::<Object>()
            .map_err(|_| VerifyFailure::UnsupportedKey)
    };
    let jwk =
        to_object(serde_wasm_bindgen::to_value(key).map_err(|_| VerifyFailure::UnsupportedKey)?)?;
    let import = to_object(
        serde_wasm_bindgen::to_value(&EcAlgorithm {
            name: "ECDSA",
            named_curve: Some("P-256"),
            hash: None,
        })
        .map_err(|_| VerifyFailure::UnsupportedKey)?,
    )?;
    let verification = to_object(
        serde_wasm_bindgen::to_value(&EcAlgorithm {
            name: "ECDSA",
            named_curve: None,
            hash: Some("SHA-256"),
        })
        .map_err(|_| VerifyFailure::UnsupportedKey)?,
    )?;
    let usages = Array::new();
    usages.push(&JsValue::from_str("verify"));
    let public_key = JsFuture::from(
        subtle
            .import_key_with_object("jwk", &jwk, &import, false, &usages)
            .map_err(|_| VerifyFailure::UnsupportedKey)?,
    )
    .await
    .map_err(|_| VerifyFailure::UnsupportedKey)?
    .dyn_into::<web_sys::CryptoKey>()
    .map_err(|_| VerifyFailure::UnsupportedKey)?;
    let result = JsFuture::from(
        subtle
            .verify_with_object_and_u8_array_and_u8_array(
                &verification,
                &public_key,
                signature,
                signed,
            )
            .map_err(|_| VerifyFailure::Crypto)?,
    )
    .await
    .map_err(|_| VerifyFailure::Crypto)?;
    signature_validity(result)
}

fn signature_validity(result: JsValue) -> Result<SignatureValidity, VerifyFailure> {
    match result.as_bool() {
        Some(true) => Ok(SignatureValidity::Valid),
        Some(false) => Ok(SignatureValidity::Invalid),
        None => Err(VerifyFailure::Crypto),
    }
}
