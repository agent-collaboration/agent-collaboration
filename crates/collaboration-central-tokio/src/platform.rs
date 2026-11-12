//! Native key transport and cryptography; Access policy remains in the core.

use axum::{
    extract::{FromRequestParts, ws::WebSocketUpgrade},
    response::IntoResponse,
};
use collaboration_central_core::{
    AccessJwk, AdmissionOrigin, CentralObservation, CentralPlatform, CoreHost, KeyFetchFailure,
    RawJwks, SerializedObjectCall, SignatureValidity, UpgradeFailure, VerifyFailure,
};
use runtime_primitives::Clock;
use std::{
    future::{Future, ready},
    sync::Arc,
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum TokioPlatformError {
    #[error("Access key source must be an HTTP(S) URL")]
    InvalidKeySource,
    #[error("HTTP client could not be constructed")]
    HttpClient(#[source] reqwest::Error),
    #[error(transparent)]
    Store(#[from] durable_steps::StoreError),
}

#[derive(Clone)]
pub struct TokioCentralPlatform {
    client: reqwest::Client,
    key_source: reqwest::Url,
    clock: Arc<dyn Clock>,
    observe: Arc<dyn Fn(CentralObservation) + Send + Sync>,
    objects: Arc<crate::native_objects::NativeObjects>,
    ids: runtime_primitives::UuidV7Ids,
}

impl TokioCentralPlatform {
    pub async fn new(
        key_source: &str,
        clock: Arc<dyn Clock>,
        observe: impl Fn(CentralObservation) + Send + Sync + 'static,
        root: &std::path::Path,
        registry: fleet_sync_lease::RegistryConfig,
        delivery: crate::DeliveryConfig,
    ) -> Result<Self, TokioPlatformError> {
        let key_source =
            reqwest::Url::parse(key_source).map_err(|_| TokioPlatformError::InvalidKeySource)?;
        if !matches!(key_source.scheme(), "http" | "https") {
            return Err(TokioPlatformError::InvalidKeySource);
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(TokioPlatformError::HttpClient)?;
        let objects =
            crate::native_objects::NativeObjects::open(root, clock.clone(), registry, delivery)
                .await?;
        Ok(Self {
            client,
            key_source,
            clock,
            observe: Arc::new(observe),
            objects,
            ids: runtime_primitives::UuidV7Ids,
        })
    }

    pub async fn shutdown(&self) -> Result<(), durable_steps::StoreError> {
        self.objects.shutdown().await
    }

    pub(crate) fn begin_shutdown(&self) {
        self.objects.stop.cancel();
    }
}

impl CentralPlatform for TokioCentralPlatform {
    type Host = crate::TokioStorageHost<CoreHost<crate::TokioObjectIo>>;
    fn clock(&self) -> &dyn Clock {
        self.clock.as_ref()
    }
    fn ids(&self) -> &dyn runtime_primitives::IdSource {
        &self.ids
    }

    async fn transport_object(
        &self,
        call: SerializedObjectCall,
    ) -> Result<Vec<u8>, durable_steps::CallFailure> {
        let (address, body) = call.into_parts();
        self.objects.call(address, body).await
    }

    async fn upgrade_session(
        &self,
        mut request: axum::http::request::Parts,
        binding: fleet_sync_session::ConnectionBinding,
        admission_origin: AdmissionOrigin,
    ) -> Result<axum::http::Response<axum::body::Body>, UpgradeFailure> {
        if self.objects.stop.is_cancelled() {
            return Err(UpgradeFailure::Refused);
        }
        let upgrade = WebSocketUpgrade::from_request_parts(&mut request, &())
            .await
            .map_err(|_| UpgradeFailure::NotAnUpgrade)?;
        let objects = self.objects.clone();
        let token = objects.tasks.token();
        Ok(upgrade
            .on_upgrade(move |socket| async move {
                let _token = token;
                objects
                    .serve_socket(socket, binding, admission_origin)
                    .await;
            })
            .into_response())
    }

    async fn fetch_access_keys(&self) -> Result<RawJwks, KeyFetchFailure> {
        let response = self
            .client
            .get(self.key_source.clone())
            .send()
            .await
            .map_err(|_| KeyFetchFailure::Unreachable)?;
        let status = response.status();
        let response = response
            .error_for_status()
            .map_err(|_| KeyFetchFailure::BadResponse {
                status: status.as_u16(),
            })?;
        let cache_control = response
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response
            .bytes()
            .await
            .map_err(|_| KeyFetchFailure::Unreachable)?
            .to_vec();
        Ok(RawJwks {
            body,
            cache_control,
        })
    }

    fn verify_rs256(
        &self,
        key: &AccessJwk,
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send {
        ready(crate::access_crypto::verify_rs256_signature(
            key, signed, signature,
        ))
    }

    fn observe(&self, observation: CentralObservation) {
        (self.observe)(observation);
    }
}
