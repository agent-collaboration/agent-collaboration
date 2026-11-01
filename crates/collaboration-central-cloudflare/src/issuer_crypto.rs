//! Standard Ed25519 WebCrypto, with malformed public inputs refused as Invalid.
use collaboration_central_core::{SignatureValidity, VerifyFailure};
use js_sys::{Array, Reflect, Uint8Array, futures::JsFuture};
use worker::wasm_bindgen::{JsCast, JsValue};

pub(crate) async fn verify_ed25519_signature(
    public_key: &[u8],
    signed: &[u8],
    signature: &[u8],
) -> Result<SignatureValidity, VerifyFailure> {
    if public_key.len() != 32 || signature.len() != 64 {
        return Ok(SignatureValidity::Invalid);
    }
    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    let crypto = global.crypto().map_err(|_| VerifyFailure::Crypto)?.subtle();
    let usages = Array::new();
    usages.push(&JsValue::from_str("verify"));
    let public_bytes = Uint8Array::from(public_key);
    let imported =
        match crypto.import_key_with_str("raw", public_bytes.as_ref(), "Ed25519", false, &usages) {
            Ok(imported) => imported,
            Err(error) => return refused_import(error),
        };
    let imported = match JsFuture::from(imported).await {
        Ok(imported) => imported,
        Err(error) => return refused_import(error),
    };
    let key = imported
        .dyn_into::<web_sys::CryptoKey>()
        .map_err(|_| VerifyFailure::Crypto)?;
    let verified = JsFuture::from(
        crypto
            .verify_with_str_and_u8_array_and_u8_array("Ed25519", &key, signature, signed)
            .map_err(|_| VerifyFailure::Crypto)?,
    )
    .await
    .map_err(|_| VerifyFailure::Crypto)?;
    match verified.as_bool() {
        Some(true) => Ok(SignatureValidity::Valid),
        Some(false) => Ok(SignatureValidity::Invalid),
        None => Err(VerifyFailure::Crypto),
    }
}

fn refused_import(error: JsValue) -> Result<SignatureValidity, VerifyFailure> {
    let name = Reflect::get(&error, &JsValue::from_str("name"))
        .map_err(|_| VerifyFailure::Crypto)?
        .as_string();
    // DataError means rejected public key material. An unsupported algorithm
    // or an unavailable runtime remains a capability failure, never Invalid.
    match name.as_deref() {
        Some("DataError") => Ok(SignatureValidity::Invalid),
        _ => Err(VerifyFailure::Crypto),
    }
}
