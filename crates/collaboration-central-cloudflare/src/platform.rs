//! Worker Access capabilities; verification and key-cache policy live in the core.

use crate::access_verification::{InvocationLabels, VerificationLayer};
use collaboration_central_core::{
    AccessJwk, CentralObservation, CentralPlatform, KeyFetchFailure, RawJwks, SignatureValidity,
    VerifyFailure,
};
use runtime_primitives::{Clock, SyncTime};
use std::future::Future;
use std::sync::Arc;
use worker::{Env, send::SendWrapper};
use worker::{Fetch, send::SendFuture};

#[derive(Clone)]
pub(crate) struct CloudflareCentralPlatform {
    key_source: String,
    labels: InvocationLabels,
    layer: VerificationLayer,
    clock: CloudflareClock,
    env: Arc<SendWrapper<Env>>,
}

#[derive(Clone, Copy)]
pub(crate) struct CloudflareClock;

impl Clock for CloudflareClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(chrono::Utc::now().timestamp_millis())
    }
}
impl runtime_primitives::IdSource for CloudflareClock {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}

impl CloudflareCentralPlatform {
    pub(crate) fn new(
        key_source: String,
        labels: InvocationLabels,
        layer: VerificationLayer,
        env: Env,
    ) -> Self {
        Self {
            key_source,
            labels,
            layer,
            clock: CloudflareClock,
            env: Arc::new(SendWrapper::new(env)),
        }
    }
}

impl CentralPlatform for CloudflareCentralPlatform {
    type Host = crate::CloudflareStorageHost<
        collaboration_central_core::CoreHost<crate::object_shell::CloudflareObjectIo>,
    >;
    fn clock(&self) -> &dyn Clock {
        &self.clock
    }
    fn ids(&self) -> &dyn runtime_primitives::IdSource {
        &self.clock
    }
    fn transport_object(
        &self,
        call: collaboration_central_core::SerializedObjectCall,
    ) -> impl Future<Output = Result<Vec<u8>, durable_steps::CallFailure>> + Send {
        SendFuture::new(async move {
            let (address, bytes) = call.into_parts();
            crate::cloudflare_transport::call(&self.env, address, bytes).await
        })
    }
    fn upgrade_session(
        &self,
        parts: http::request::Parts,
        binding: fleet_sync_session::ConnectionBinding,
        admission_origin: collaboration_central_core::AdmissionOrigin,
    ) -> impl Future<
        Output = Result<
            http::Response<axum::body::Body>,
            collaboration_central_core::UpgradeFailure,
        >,
    > + Send {
        SendFuture::new(async move {
            use base64::Engine;
            use collaboration_central_core::{ObjectAddress, UpgradeFailure};
            if !parts
                .headers
                .get("upgrade")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
            {
                return Err(UpgradeFailure::NotAnUpgrade);
            }
            let headers = worker::Headers::new();
            for name in [
                "upgrade",
                "connection",
                "sec-websocket-key",
                "sec-websocket-version",
                "sec-websocket-protocol",
            ] {
                if let Some(value) = parts.headers.get(name) {
                    headers
                        .set(
                            name,
                            value.to_str().map_err(|_| UpgradeFailure::NotAnUpgrade)?,
                        )
                        .map_err(|_| UpgradeFailure::NotAnUpgrade)?;
                }
            }
            let address = ObjectAddress::Connection(admission_origin.installation.clone());
            let attachment = crate::binding_attachment::BindingAttachment {
                connection: binding.connection,
                peer: binding.peer,
                expires_at: binding.expires_at,
                registration: fleet_sync_session::ConnectionRegistration::Opening,
                admission_origin,
            };
            let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&attachment).map_err(|_| UpgradeFailure::Refused)?);
            headers
                .set("x-central-session-binding", &encoded)
                .map_err(|_| UpgradeFailure::Refused)?;
            crate::cloudflare_transport::address_header(&headers, &address)
                .map_err(|_| UpgradeFailure::Refused)?;
            let mut init = worker::RequestInit::new();
            init.with_method(worker::Method::Get).with_headers(headers);
            let request = worker::Request::new_with_init("https://central.internal/session", &init)
                .map_err(|_| UpgradeFailure::NotAnUpgrade)?;
            let response = crate::cloudflare_transport::stub(&self.env, &address)
                .map_err(|_| UpgradeFailure::Refused)?
                .fetch_with_request(request)
                .await
                .map_err(|_| UpgradeFailure::Refused)?;
            if response.status_code() != 101 {
                return Err(UpgradeFailure::Refused);
            }
            Ok(response.into())
        })
    }

    fn fetch_access_keys(&self) -> impl Future<Output = Result<RawJwks, KeyFetchFailure>> + Send {
        SendFuture::new(async {
            let mut response = Fetch::Url(
                self.key_source
                    .parse()
                    .map_err(|_| KeyFetchFailure::Unreachable)?,
            )
            .send()
            .await
            .map_err(|_| KeyFetchFailure::Unreachable)?;
            let status = response.status_code();
            if !(200..300).contains(&status) {
                return Err(KeyFetchFailure::BadResponse { status });
            }
            let cache_control = response
                .headers()
                .get("cache-control")
                .map_err(|_| KeyFetchFailure::Unreachable)?;
            let body = response
                .bytes()
                .await
                .map_err(|_| KeyFetchFailure::Unreachable)?;
            Ok(RawJwks {
                body,
                cache_control,
            })
        })
    }

    fn verify_rs256(
        &self,
        key: &AccessJwk,
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send {
        SendFuture::new(crate::access_crypto::verify_rs256_signature(
            key, signed, signature,
        ))
    }

    fn observe(&self, observation: CentralObservation) {
        self.labels.observe(self.layer, observation);
    }

    fn verify_ed25519(
        &self,
        public_key: &[u8],
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send {
        SendFuture::new(crate::issuer_crypto::verify_ed25519_signature(
            public_key, signed, signature,
        ))
    }
}
