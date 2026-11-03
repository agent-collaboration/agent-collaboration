//! Worker client-key handle imported from the host's configured secret bytes.
use collaboration_central_core::{ClientAssertionRefusal, Ed25519ClientSigner, IssuerKeyId};
use js_sys::{Array, Uint8Array, futures::JsFuture};
use std::sync::Arc;
use worker::{
    send::{SendFuture, SendWrapper},
    wasm_bindgen::{JsCast, JsValue},
};

#[derive(Clone)]
pub struct CloudflareEd25519ClientSigner {
    key_id: IssuerKeyId,
    key: Arc<SendWrapper<web_sys::CryptoKey>>,
}
impl CloudflareEd25519ClientSigner {
    pub async fn from_pkcs8(
        key_id: IssuerKeyId,
        secret: &[u8],
    ) -> Result<Self, ClientAssertionRefusal> {
        let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
        let crypto = global
            .crypto()
            .map_err(|_| ClientAssertionRefusal::Signing)?
            .subtle();
        let usages = Array::new();
        usages.push(&JsValue::from_str("sign"));
        let bytes = Uint8Array::from(secret);
        let promise = crypto
            .import_key_with_str("pkcs8", bytes.as_ref(), "Ed25519", false, &usages)
            .map_err(|_| ClientAssertionRefusal::Signing)?;
        let key = JsFuture::from(promise)
            .await
            .map_err(|_| ClientAssertionRefusal::Signing)?
            .dyn_into::<web_sys::CryptoKey>()
            .map_err(|_| ClientAssertionRefusal::Signing)?;
        Ok(Self {
            key_id,
            key: Arc::new(SendWrapper::new(key)),
        })
    }
}
impl Ed25519ClientSigner for CloudflareEd25519ClientSigner {
    fn key_id(&self) -> &str {
        self.key_id.as_str()
    }
    fn sign_client_assertion(
        &self,
        signed: &[u8],
    ) -> impl std::future::Future<Output = Result<Vec<u8>, ClientAssertionRefusal>> + Send {
        SendFuture::new(async move {
            let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
            let crypto = global
                .crypto()
                .map_err(|_| ClientAssertionRefusal::Signing)?
                .subtle();
            let promise = crypto
                .sign_with_str_and_u8_array("Ed25519", &self.key, signed)
                .map_err(|_| ClientAssertionRefusal::Signing)?;
            let signed = JsFuture::from(promise)
                .await
                .map_err(|_| ClientAssertionRefusal::Signing)?;
            Ok(Uint8Array::new(&signed).to_vec())
        })
    }
}
