//! WebCrypto mechanics after the core has selected an allowed signing key.

use collaboration_central_core::{AccessJwk, SignatureValidity, VerifyFailure};
use js_sys::{Array, Object, futures::JsFuture};
use serde::Serialize;
use worker::wasm_bindgen::{JsCast, JsValue};

#[derive(Serialize)]
struct ImportAlgorithm {
    name: &'static str,
    hash: &'static str,
}

pub(crate) async fn verify_rs256_signature(
    key: &AccessJwk,
    signed: &[u8],
    signature: &[u8],
) -> Result<SignatureValidity, VerifyFailure> {
    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    let crypto = global.crypto().map_err(|_| VerifyFailure::Crypto)?.subtle();
    let object = |value: JsValue| {
        value
            .dyn_into::<Object>()
            .map_err(|_| VerifyFailure::UnsupportedKey)
    };
    let jwk =
        object(serde_wasm_bindgen::to_value(key).map_err(|_| VerifyFailure::UnsupportedKey)?)?;
    let algorithm = object(
        serde_wasm_bindgen::to_value(&ImportAlgorithm {
            name: "RSASSA-PKCS1-v1_5",
            hash: "SHA-256",
        })
        .map_err(|_| VerifyFailure::UnsupportedKey)?,
    )?;
    let usages = Array::new();
    usages.push(&JsValue::from_str("verify"));
    let imported = JsFuture::from(
        crypto
            .import_key_with_object("jwk", &jwk, &algorithm, false, &usages)
            .map_err(|_| VerifyFailure::UnsupportedKey)?,
    )
    .await
    .map_err(|_| VerifyFailure::UnsupportedKey)?;
    let key = imported
        .dyn_into::<web_sys::CryptoKey>()
        .map_err(|_| VerifyFailure::UnsupportedKey)?;
    let result = JsFuture::from(
        crypto
            .verify_with_str_and_u8_array_and_u8_array("RSASSA-PKCS1-v1_5", &key, signature, signed)
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
